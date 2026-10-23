use cssparser::{Parser, UnicodeRange};
use moli_crypto::sha256_hex;
use moli_css_parse::{
    DeclarationParseOptions, parse_declaration_list, parse_font_face_rule_view_with_stylo,
    parse_font_faces, parse_stylesheet_rule_snapshots_with_stylo,
};
use moli_layout::{WebFontFace, WebFontUnicodeRange};
use url::Url;

use crate::protocol_types::OptionalResourceFetchMask;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StylesheetLoadBlockingResourceKind {
    Image,
    Font,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StylesheetLoadBlockingResource {
    request_url: Option<Url>,
    kind: StylesheetLoadBlockingResourceKind,
    web_font: Option<StylesheetWebFont>,
}

impl StylesheetLoadBlockingResource {
    fn new(request_url: Url, kind: StylesheetLoadBlockingResourceKind) -> Self {
        Self {
            request_url: Some(request_url),
            kind,
            web_font: None,
        }
    }

    pub(crate) fn image(request_url: Url) -> Self {
        Self::new(request_url, StylesheetLoadBlockingResourceKind::Image)
    }

    pub(crate) fn font(request_url: Option<Url>, web_font: StylesheetWebFont) -> Self {
        Self {
            request_url,
            kind: StylesheetLoadBlockingResourceKind::Font,
            web_font: Some(web_font),
        }
    }

    pub(crate) fn request_url(&self) -> Option<&Url> {
        self.request_url.as_ref()
    }

    pub(crate) fn with_request_url(mut self, url: Url) -> Self {
        self.request_url = Some(url);
        self
    }

    pub(crate) fn into_parts(self) -> (Option<Url>, Option<StylesheetWebFont>) {
        (self.request_url, self.web_font)
    }

    pub(crate) fn web_font(&self) -> Option<&StylesheetWebFont> {
        self.web_font.as_ref()
    }

    pub(crate) fn bind_web_font_request(mut self, request_id: u64) -> Self {
        self.web_font = self
            .web_font
            .take()
            .map(|font| font.bind_request(request_id));
        self
    }

    pub(crate) fn kind(&self) -> StylesheetLoadBlockingResourceKind {
        self.kind
    }
}

/// Parsed `@font-face` metadata that stays attached to the exact resource
/// request until its owner-validated completion.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StylesheetWebFont {
    slot: String,
    face: WebFontFace,
    sources: Vec<moli_css_parse::CssFontSource>,
    request_id: Option<u64>,
}

impl StylesheetWebFont {
    pub(crate) fn bind_request(mut self, request_id: u64) -> Self {
        self.request_id = Some(request_id);
        self
    }

    pub(crate) fn request_id(&self) -> Option<u64> {
        self.request_id
    }

    pub(crate) fn sources(&self) -> &[moli_css_parse::CssFontSource] {
        &self.sources
    }

    pub(crate) fn slot(&self) -> &str {
        &self.slot
    }

    pub(crate) fn face(&self) -> &WebFontFace {
        &self.face
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CompletedStylesheetWebFont {
    request: StylesheetWebFont,
    bytes: Option<Vec<u8>>,
}

impl CompletedStylesheetWebFont {
    pub(crate) fn response(request: StylesheetWebFont, bytes: Vec<u8>) -> Self {
        Self {
            request,
            bytes: Some(bytes),
        }
    }

    pub(crate) fn failure(request: StylesheetWebFont) -> Self {
        Self {
            request,
            bytes: None,
        }
    }

    pub(crate) fn into_parts(self) -> (StylesheetWebFont, Option<Vec<u8>>) {
        (self.request, self.bytes)
    }
}

/// Discovers load-blocking web fonts from a newly completed stylesheet.
///
/// CSS images deliberately do not use this text-scanning path. Paint layout
/// reports the computed image URLs of boxes that actually participate in the
/// rendered tree, so unmatched rules and `display: none` subtrees never start
/// image requests merely because their URL appears in stylesheet text.
pub(crate) fn stylesheet_load_blocking_font_resources(
    css_text: &str,
    base_url: &Url,
    optional_resource_fetch_mask: OptionalResourceFetchMask,
) -> Vec<StylesheetLoadBlockingResource> {
    if !optional_resource_fetch_mask.contains(OptionalResourceFetchMask::FONT) {
        return Vec::new();
    }
    stylesheet_web_font_resources(css_text, base_url)
}

fn stylesheet_web_font_resources(
    css_text: &str,
    base_url: &Url,
) -> Vec<StylesheetLoadBlockingResource> {
    let rules = parse_stylesheet_rule_snapshots_with_stylo(css_text);
    let mut font_face_rules = Vec::new();
    collect_font_face_rule_css(&rules, &mut font_face_rules);
    font_face_rules
        .into_iter()
        .filter_map(|css_text| stylesheet_web_font_resource(&css_text, base_url))
        .collect()
}

/// Projects one already-parsed and serialized `@font-face` rule into the
/// typed request metadata retained by the style world.
pub(crate) fn stylesheet_web_font_resource(
    css_text: &str,
    base_url: &Url,
) -> Option<StylesheetLoadBlockingResource> {
    let descriptor = parsed_web_font_face(css_text)?;
    let sources = moli_css_parse::parse_font_face_sources(descriptor.source())?
        .into_iter()
        .filter_map(|source| match source {
            moli_css_parse::CssFontSource::Local(name) => {
                Some(moli_css_parse::CssFontSource::Local(name))
            }
            moli_css_parse::CssFontSource::Url(url) => resolve_font_source_url(&url, base_url)
                .map(|url| moli_css_parse::CssFontSource::Url(url.to_string())),
        })
        .collect();
    stylesheet_web_font_resource_with_resolved_sources(css_text, sources)
}

/// Projects a native Stylo `@font-face` rule using the URL already resolved
/// in that rule's own parser context.
///
/// Imported rules retain their imported stylesheet's URL context inside
/// `SpecifiedUrl`. Callers must use this entry point instead of resolving the
/// serialized, relative `src` value against the root stylesheet again.
pub(crate) fn stylesheet_web_font_resource_with_resolved_sources(
    css_text: &str,
    sources: Vec<moli_css_parse::CssFontSource>,
) -> Option<StylesheetLoadBlockingResource> {
    if sources.is_empty() {
        return None;
    }
    // A font declaration can resolve entirely through local() without ever
    // producing a URL. Admission selects a network source only if needed.
    let request_url = sources.iter().find_map(|source| match source {
        moli_css_parse::CssFontSource::Url(url) => Url::parse(url).ok(),
        moli_css_parse::CssFontSource::Local(_) => None,
    });
    let descriptor = parsed_web_font_face(css_text)?;
    Some(stylesheet_web_font_resource_with_descriptor(
        css_text,
        request_url,
        descriptor,
        sources,
    ))
}

fn stylesheet_web_font_resource_with_descriptor(
    css_text: &str,
    request_url: Option<Url>,
    descriptor: ParsedWebFontFace,
    sources: Vec<moli_css_parse::CssFontSource>,
) -> StylesheetLoadBlockingResource {
    // The slot identifies a shareable resource, not a layout generation or
    // an individual declaration. The import response and retained style world
    // must converge on one slot. Identical relative declarations parsed under
    // different stylesheet bases must not
    // alias, so identity includes every resolved fallback, its kind and order.
    // Length-prefix strings so source boundaries cannot collide.
    let mut slot_material = Vec::new();
    slot_material.extend_from_slice(&(css_text.len() as u64).to_le_bytes());
    slot_material.extend_from_slice(css_text.as_bytes());
    for source in &sources {
        let (kind, value) = match source {
            moli_css_parse::CssFontSource::Local(value) => (0, value),
            moli_css_parse::CssFontSource::Url(value) => (1, value),
        };
        slot_material.push(kind);
        slot_material.extend_from_slice(&(value.len() as u64).to_le_bytes());
        slot_material.extend_from_slice(value.as_bytes());
    }
    let slot = format!("stylesheet-font:{}", sha256_hex(&slot_material));
    StylesheetLoadBlockingResource::font(
        request_url,
        StylesheetWebFont {
            slot,
            face: descriptor.into_face(),
            sources,
            request_id: None,
        },
    )
}

fn collect_font_face_rule_css(rules: &[moli_css_parse::CssRuleSnapshot], output: &mut Vec<String>) {
    for rule in rules {
        if rule.rule_type == style::stylesheets::CssRuleType::FontFace {
            output.push(rule.css_text.clone());
        }
        collect_font_face_rule_css(&rule.child_rules, output);
    }
}

#[derive(Debug)]
struct ParsedWebFontFace {
    family: String,
    source: String,
    weight: String,
    stretch: String,
    style: String,
    unicode_ranges: Vec<WebFontUnicodeRange>,
}

impl ParsedWebFontFace {
    fn source(&self) -> &str {
        &self.source
    }

    fn into_face(self) -> WebFontFace {
        WebFontFace::new(self.family)
            .with_css_descriptors(&self.weight, &self.stretch, &self.style)
            .with_unicode_ranges(self.unicode_ranges)
    }
}

fn parsed_web_font_face(css_text: &str) -> Option<ParsedWebFontFace> {
    let parsed = parse_font_faces(css_text).into_iter().next()?;
    let view = parse_font_face_rule_view_with_stylo(css_text)?;
    let declarations = parse_declaration_list(
        &view.style_text,
        DeclarationParseOptions {
            canonicalize_property_name: true,
            unescape_value_semicolons: false,
            preserve_empty_values: false,
        },
    );
    let descriptor = |name: &str| {
        declarations
            .iter()
            .rev()
            .find(|declaration| declaration.name == name)
            .map(|declaration| declaration.value.as_str())
    };
    Some(ParsedWebFontFace {
        family: parsed.family,
        source: parsed.source,
        weight: descriptor("font-weight").unwrap_or("normal").to_owned(),
        stretch: descriptor("font-stretch").unwrap_or("normal").to_owned(),
        style: descriptor("font-style").unwrap_or("normal").to_owned(),
        unicode_ranges: descriptor("unicode-range")
            .and_then(parse_font_unicode_ranges)
            .unwrap_or_default(),
    })
}

pub(crate) fn parse_font_unicode_ranges(value: &str) -> Option<Vec<WebFontUnicodeRange>> {
    let input = value;
    let mut input = Parser::new(input);
    input
        .parse_comma_separated(|input| {
            let range = UnicodeRange::parse(input)?;
            Ok::<_, cssparser::ParseError<()>>(WebFontUnicodeRange::new(range.start, range.end))
        })
        .ok()
}

pub(crate) use moli_css_parse::legacy_variations_font_format;

fn resolve_font_source_url(raw_url: &str, base_url: &Url) -> Option<Url> {
    let raw_url = raw_url.trim();
    let url = base_url
        .join(raw_url)
        .or_else(|_| Url::parse(raw_url))
        .ok()?;
    matches!(url.scheme(), "http" | "https" | "data" | "blob").then_some(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use moli_layout::WebFontStyle;

    fn base_url() -> Url {
        Url::parse("https://example.test/assets/app.css").expect("base URL should parse")
    }

    #[test]
    fn local_only_declarations_do_not_require_a_network_url() {
        let resources = stylesheet_load_blocking_font_resources(
            r#"@font-face { font-family: LocalOnly; src: local("First"), local("Second"); }"#,
            &base_url(),
            OptionalResourceFetchMask::FONT,
        );
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].request_url(), None);
        let font = resources[0].web_font().expect("local-only declaration");
        assert_eq!(font.face().family_name(), "LocalOnly");
        assert_eq!(
            font.sources(),
            [
                moli_css_parse::CssFontSource::Local("First".into()),
                moli_css_parse::CssFontSource::Local("Second".into()),
            ]
        );
    }

    #[test]
    fn selects_one_supported_source_per_font_face_and_keeps_descriptors() {
        let resources = stylesheet_load_blocking_font_resources(
            r#"
                @font-face {
                    font-family: Demo;
                    src: local("Demo"),
                         url(fonts/demo.svg) format("svg"),
                         url(fonts/demo.woff2) format("woff2"),
                         url("/fonts/demo.woff") format("woff");
                    font-weight: 625 800;
                    font-stretch: semi-condensed expanded;
                    font-style: oblique 30deg 45deg;
                    unicode-range: U+0000-00FF, U+4E??;
                }
                @font-face {
                    font-family: DemoBold;
                    src: url(fonts/demo.woff2);
                }
                @media screen {
                    @font-face {
                        font-family: NestedDemo;
                        src: url(fonts/nested.woff2);
                    }
                }
                @font-face {
                    font-family: DataFace;
                    src: url("data:font/woff2;base64,AA==") format("woff2");
                }
                body { background-image: url(hero.png); }
            "#,
            &base_url(),
            OptionalResourceFetchMask::FONT,
        );
        assert_eq!(
            resources
                .iter()
                .map(|resource| resource.request_url().expect("URL source").to_string())
                .collect::<Vec<_>>(),
            vec![
                "https://example.test/assets/fonts/demo.woff2",
                "https://example.test/assets/fonts/demo.woff2",
                "https://example.test/assets/fonts/nested.woff2",
                "data:font/woff2;base64,AA==",
            ]
        );
        let web_font = resources[0]
            .web_font
            .as_ref()
            .expect("font resource should retain its descriptors");
        assert!(web_font.slot().starts_with("stylesheet-font:"));
        assert_eq!(web_font.request_id(), None);
        assert_eq!(web_font.face().family_name(), "Demo");
        assert_eq!(web_font.face().weight(), Some(625.0));
        assert_eq!(web_font.face().stretch(), Some(87.5));
        assert_eq!(
            web_font.face().style(),
            Some(WebFontStyle::Oblique(Some(30.0)))
        );
        assert_eq!(
            web_font.face().unicode_ranges(),
            [
                WebFontUnicodeRange::new(0x0000, 0x00ff),
                WebFontUnicodeRange::new(0x4e00, 0x4eff),
            ]
        );
    }

    #[test]
    fn invalid_unicode_range_falls_back_to_the_full_font_face_range() {
        let resources = stylesheet_load_blocking_font_resources(
            r#"
                @font-face {
                    font-family: Demo;
                    src: url(fonts/demo.woff2);
                    unicode-range: U+110000-120000;
                }
            "#,
            &base_url(),
            OptionalResourceFetchMask::FONT,
        );
        assert_eq!(resources.len(), 1);
        assert!(
            resources[0]
                .web_font()
                .expect("font metadata")
                .face()
                .unicode_ranges()
                .is_empty()
        );
    }

    #[test]
    fn native_resolved_font_projection_converges_with_import_response_slot() {
        let css_text = r#"
            @font-face {
                font-family: Imported;
                src: local("Imported"),
                     url("./fonts/imported.svg") format("svg"),
                     url("./fonts/imported.woff2") format("woff2");
                font-weight: 625 800;
                unicode-range: U+20-7E;
            }
        "#;
        let imported_base =
            Url::parse("https://example.test/theme/imported.css").expect("imported base URL");
        let resolved = Url::parse("https://example.test/theme/fonts/imported.woff2")
            .expect("resolved native font URL");

        let response_projection = stylesheet_web_font_resource(css_text, &imported_base)
            .expect("the import response should project its font");
        let native_projection = stylesheet_web_font_resource_with_resolved_sources(
            css_text,
            vec![
                moli_css_parse::CssFontSource::Local("Imported".to_owned()),
                moli_css_parse::CssFontSource::Url(resolved.to_string()),
            ],
        )
        .expect("the retained native rule should project its resolved font");

        assert_eq!(response_projection.request_url(), Some(&resolved));
        assert_eq!(native_projection.request_url(), Some(&resolved));
        assert_eq!(
            response_projection
                .web_font()
                .expect("response font")
                .slot(),
            native_projection.web_font().expect("native font").slot(),
            "the early import-response registration and retained manifest must keep one slot",
        );
        assert_eq!(
            response_projection
                .web_font()
                .expect("response font")
                .face(),
            native_projection.web_font().expect("native font").face(),
        );
    }

    #[test]
    fn resolved_request_url_keeps_identical_relative_rules_in_distinct_slots() {
        let css_text = "@font-face { font-family: Shared; src: url(./fonts/shared.woff2); }";
        let first_url = Url::parse("https://example.test/first/fonts/shared.woff2").unwrap();
        let second_url = Url::parse("https://example.test/second/fonts/shared.woff2").unwrap();
        let first = stylesheet_web_font_resource_with_resolved_sources(
            css_text,
            vec![moli_css_parse::CssFontSource::Url(first_url.to_string())],
        )
        .expect("first native resource");
        let second = stylesheet_web_font_resource_with_resolved_sources(
            css_text,
            vec![moli_css_parse::CssFontSource::Url(second_url.to_string())],
        )
        .expect("second native resource");

        assert_ne!(
            first.web_font().expect("first font").slot(),
            second.web_font().expect("second font").slot(),
            "equal relative text parsed in different stylesheet directories must not alias",
        );
    }

    #[test]
    fn font_slot_includes_every_resolved_fallback_in_source_order() {
        use moli_css_parse::CssFontSource::{Local, Url as SourceUrl};
        let css =
            "@font-face{font-family:Fallback;src:url(data:font/woff2;base64,YmFk),url(font.woff2)}";
        let first = stylesheet_web_font_resource(
            css,
            &Url::parse("https://example.test/a/style.css").unwrap(),
        )
        .unwrap();
        let second = stylesheet_web_font_resource(
            css,
            &Url::parse("https://example.test/b/style.css").unwrap(),
        )
        .unwrap();
        assert_eq!(first.request_url(), second.request_url());
        assert_ne!(
            first.web_font().unwrap().slot(),
            second.web_font().unwrap().slot()
        );
        let slot = |sources| {
            stylesheet_web_font_resource_with_resolved_sources(css, sources)
                .unwrap()
                .web_font()
                .unwrap()
                .slot()
                .to_owned()
        };
        let url = "https://example.test/font.woff2".to_owned();
        let other = "https://example.test/other.woff2".to_owned();
        assert_ne!(
            slot(vec![SourceUrl(url.clone()), SourceUrl(other.clone())]),
            slot(vec![SourceUrl(other.clone()), SourceUrl(url.clone())]),
        );
        assert_ne!(
            slot(vec![SourceUrl(url.clone()), Local(other.clone())]),
            slot(vec![SourceUrl(url), SourceUrl(other)]),
        );
    }

    #[test]
    fn load_blocking_font_selection_ignores_image_and_unrelated_bits() {
        let css = r#"
            @font-face {
                font-family: Demo;
                src: url(fonts/demo.woff2);
            }
            body { background-image: url(images/hero.png); }
        "#;
        let selected = |mask| {
            stylesheet_load_blocking_font_resources(css, &base_url(), mask)
                .into_iter()
                .map(|resource| {
                    (
                        resource.kind(),
                        resource.request_url().expect("URL source").to_string(),
                    )
                })
                .collect::<Vec<_>>()
        };

        assert!(selected(OptionalResourceFetchMask::NONE).is_empty());
        assert!(selected(OptionalResourceFetchMask::IMAGE).is_empty());
        assert_eq!(
            selected(OptionalResourceFetchMask::FONT),
            vec![(
                StylesheetLoadBlockingResourceKind::Font,
                "https://example.test/assets/fonts/demo.woff2".to_owned(),
            )]
        );
        assert_eq!(
            selected(OptionalResourceFetchMask::IMAGE | OptionalResourceFetchMask::FONT),
            vec![(
                StylesheetLoadBlockingResourceKind::Font,
                "https://example.test/assets/fonts/demo.woff2".to_owned(),
            )]
        );
        for unrelated in [
            OptionalResourceFetchMask::AUDIO,
            OptionalResourceFetchMask::VIDEO,
            OptionalResourceFetchMask::MEDIA,
            OptionalResourceFetchMask::TEXT_TRACK,
        ] {
            assert!(
                selected(unrelated).is_empty(),
                "{unrelated:?} must not enable stylesheet font requests"
            );
        }
    }

    #[test]
    fn legacy_variations_format_hints_select_their_container_format() {
        let resources = stylesheet_load_blocking_font_resources(
            r#"
                @font-face { font-family: Variable; src: url(fonts/variable.woff2) format('woff2-variations'); }
                @font-face { font-family: VariableTtf;
                    src: url(fonts/variable.ttf) format("truetype-variations"),
                         url(fonts/fallback.ttf) format("truetype"); }
                @font-face { font-family: Unknown; src: url(fonts/unknown.bin) format("svg-variations"); }
            "#,
            &base_url(),
            OptionalResourceFetchMask::FONT,
        );
        assert_eq!(
            resources
                .iter()
                .map(|resource| resource.request_url().expect("URL source").to_string())
                .collect::<Vec<_>>(),
            vec![
                "https://example.test/assets/fonts/variable.woff2",
                "https://example.test/assets/fonts/variable.ttf",
            ]
        );
    }
}
