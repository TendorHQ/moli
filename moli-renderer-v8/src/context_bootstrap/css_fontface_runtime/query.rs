use super::storage::font_face_set_faces_array;
use super::*;
use crate::util::serialize_v8_iter_array;

mod selection;

struct Candidate<'s> {
    face: v8::Local<'s, v8::Value>,
    family: String,
    selection: selection::Selection,
    ranges: Vec<moli_layout::WebFontUnicodeRange>,
}

pub(super) fn font_face_set_matching_faces_array<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
    query: &str,
    text: &str,
) -> Option<v8::Local<'s, v8::Array>> {
    if !moli_css_parse::font_load_query_is_valid(query) {
        return None;
    }
    // System font shorthands are valid queries, but Stylo's Servo configuration
    // does not expose their platform families for matching authored faces.
    let Some(query) = moli_css_parse::parse_font_shorthand(query) else {
        return serialize_v8_iter_array(scope, std::iter::empty::<v8::Local<'s, v8::Value>>());
    };
    let requested = selection::Selection::new(&query.weight, &query.stretch, &query.style);
    let faces = font_face_set_faces_array(scope, object)?;
    let mut candidates = Vec::new();
    for index in 0..faces.length() {
        let Some(face) = faces.get_index(scope, index) else {
            continue;
        };
        let Ok(face_object) = v8::Local::<v8::Object>::try_from(face) else {
            continue;
        };
        let family = super::loading::string_slot(scope, face_object, FONT_FACE_FAMILY_SLOT);
        let weight = super::loading::string_slot(scope, face_object, FONT_FACE_WEIGHT_SLOT);
        let stretch = super::loading::string_slot(scope, face_object, FONT_FACE_STRETCH_SLOT);
        let style = super::loading::string_slot(scope, face_object, FONT_FACE_STYLE_SLOT);
        let ranges = super::loading::string_slot(scope, face_object, FONT_FACE_UNICODE_RANGE_SLOT);
        candidates.push(Candidate {
            face,
            family: moli_css_parse::unquote_css_string(&family),
            selection: selection::Selection::new(&weight, &stretch, &style),
            ranges: crate::css_resource_urls::parse_font_unicode_ranges(&ranges)
                .unwrap_or_default(),
        });
    }
    let text = text.chars().collect::<Vec<_>>();
    let mut matching = Vec::new();
    for family in query.families {
        let family_faces = candidates
            .iter()
            .filter(|face| family.eq_ignore_ascii_case(&face.family))
            .collect::<Vec<_>>();
        let Some(best) = family_faces
            .iter()
            .min_by(|a, b| a.selection.compare(b.selection, requested))
        else {
            continue;
        };
        for face in &family_faces {
            // Select capabilities from the entire family first. Filtering by
            // text or load state earlier could choose an unrelated fallback
            // weight/style instead of the CSS-matched segmented face.
            if face.selection.compare(best.selection, requested).is_eq()
                && text.iter().any(|&character| {
                    face.ranges.is_empty()
                        || face.ranges.iter().any(|range| range.contains(character))
                })
            {
                matching.push(face.face);
            }
        }
    }
    serialize_v8_iter_array(scope, matching)
}

pub(super) fn make_rejected_dom_exception_promise<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    name: &str,
    message: &str,
) -> v8::Local<'s, v8::Promise> {
    let resolver = v8::PromiseResolver::new(scope).expect("resolver");
    let exception = crate::context_bootstrap::new_dom_exception_value(scope, message, name);
    let _ = resolver.reject(scope, exception);
    resolver.get_promise(scope)
}
