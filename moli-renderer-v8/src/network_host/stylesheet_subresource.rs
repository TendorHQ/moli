use super::subresource::{PreparedSubresourceFetch, SubresourceFetchOptions};
use super::*;
use crate::{
    css_resource_urls::{
        CompletedStylesheetWebFont, StylesheetLoadBlockingResource,
        StylesheetLoadBlockingResourceKind,
    },
    frame_owner_model::StylesheetSubresourceLoadDelayBinding,
    native_bridge::{JsContextHost, OwnerDispatchScope},
    types::SubresourceRequestInitiatorType,
};

#[derive(Debug)]
pub(crate) enum StylesheetSubresourceFetchStart {
    Pending,
    /// The request reached a synchronous terminal and has no Document
    /// font state to commit.
    Settled,
    /// A Document font reached a synchronous terminal. Network failure
    /// is represented by `CompletedStylesheetWebFont` with no body bytes.
    WebFontSettled(CompletedStylesheetWebFont),
}

pub(crate) fn start_stylesheet_subresource_fetch(
    scope: &mut v8::PinScope<'_, '_>,
    host: &mut JsContextHost,
    binding: StylesheetSubresourceLoadDelayBinding,
    resource: StylesheetLoadBlockingResource,
    css_image: Option<crate::native_bridge::CssImageResourceRequestIdentity>,
) -> Result<StylesheetSubresourceFetchStart, String> {
    if !host.stylesheet_subresource_load_delay_is_current(binding) {
        return Err("stylesheet subresource document owner is stale".to_owned());
    }
    let owner = binding
        .child_handle()
        .map_or(OwnerDispatchScope::Top, OwnerDispatchScope::Child);
    let resource_loader = host
        .document_resource_loader_for_owner(binding.owner())
        .ok_or_else(|| "stylesheet Document resource authority is unavailable".to_owned())?;
    let environment = host
        .subresource_request_environment(&resource_loader, owner)
        .ok_or_else(|| "stylesheet Document request environment is unavailable".to_owned())?;
    let resource_kind = resource.kind();
    if css_image.is_some() && resource_kind != StylesheetLoadBlockingResourceKind::Image {
        return Err("a stylesheet CSS image identity was bound to a non-image resource".to_owned());
    }
    let is_web_font = resource_kind == StylesheetLoadBlockingResourceKind::Font;
    if is_web_font {
        let web_font = resource.web_font().ok_or_else(|| {
            "Document stylesheet font is missing parsed @font-face metadata".to_owned()
        })?;
        if web_font.request_id().is_none() {
            return Err(
                "Document stylesheet font is missing its admitted request identity".to_owned(),
            );
        }
    }
    let (request_url, web_font) = resource.into_parts();
    let request_url = request_url
        .ok_or_else(|| "admitted stylesheet subresource is missing its network URL".to_owned())?;
    let options = match resource_kind {
        StylesheetLoadBlockingResourceKind::Image => SubresourceFetchOptions::CSS_IMAGE,
        StylesheetLoadBlockingResourceKind::Font => SubresourceFetchOptions::FONT,
    };
    let resource_type = options.resource_type;

    if let Some(response) = local_url_response(&request_url) {
        let mut response: crate::protocol_types::NavigationResponse = response.into();
        let encoded = (resource_kind == StylesheetLoadBlockingResourceKind::Image).then(|| {
            let manager = host
                .document_resource_loader_for_owner(binding.owner())
                .map_or_else(
                    moli_parkable_image::ParkableImageManager::default,
                    |loader| {
                        let runner = loader.task_runner();
                        loader.request_client().parkable_image_manager(&runner)
                    },
                );
            manager.from_frozen_bytes(response.take_body_bytes())
        });
        if let Some(identity) = css_image.as_ref() {
            let descriptor = crate::network_host::image_response_descriptor_from_parkable(
                &response,
                encoded
                    .as_ref()
                    .expect("a local CSS image must have parkable bytes"),
            );
            let _ = host.complete_stylesheet_css_image_response(
                identity,
                descriptor,
                encoded
                    .as_ref()
                    .expect("a local CSS image must have parkable bytes")
                    .clone(),
            );
        }
        let terminal = if is_web_font {
            let font = web_font.expect("web-font metadata was validated before consumption");
            StylesheetSubresourceFetchStart::WebFontSettled(
                if (200..=299).contains(&response.status) {
                    CompletedStylesheetWebFont::response(font, response.clone_body_bytes())
                } else {
                    CompletedStylesheetWebFont::failure(font)
                },
            )
        } else {
            StylesheetSubresourceFetchStart::Settled
        };
        let result = Ok(response);
        if let Some(encoded) = encoded {
            host.record_get_subresource_network_result_with_body_and_initiator(
                environment.frame_id,
                environment.document_url,
                request_url,
                resource_type,
                SubresourceRequestInitiatorType::Css,
                &result,
                crate::types::SubresourceResponseBody::from_parkable_image(encoded),
            );
        } else {
            host.record_get_subresource_network_result_with_initiator(
                environment.frame_id,
                environment.document_url,
                request_url,
                resource_type,
                SubresourceRequestInitiatorType::Css,
                &result,
            );
        }
        host.settle_stylesheet_subresource_load_delay(binding);
        return Ok(terminal);
    }

    if !resource_loader
        .request_client()
        .optional_resource_fetch_enabled(resource_type)
    {
        if let Some(identity) = css_image.as_ref() {
            let _ = host.fail_stylesheet_css_image(identity);
        }
        host.settle_stylesheet_subresource_load_delay(binding);
        return Ok(synchronous_failure_terminal(is_web_font, web_font));
    }
    let prepared = PreparedSubresourceFetch::prepare(
        scope,
        host,
        owner,
        &resource_loader,
        environment,
        request_url,
        options,
    )?;
    let Some(internal_id) = host.record_async_stylesheet_subresource_fetch(
        v8::Global::new(scope, scope.get_current_context()),
        binding,
        owner,
        prepared.cancel_handle.clone(),
        options.credentials_mode,
        options.request_mode,
        prepared.network_partition_key.clone(),
        prepared.network_context.policy_context,
        web_font,
        css_image,
        prepared.pending_info.clone(),
    ) else {
        return Err("stylesheet subresource owner changed before request binding".to_owned());
    };

    prepared.dispatch(
        host,
        internal_id,
        "service worker stylesheet subresource dispatch failed",
    );
    Ok(StylesheetSubresourceFetchStart::Pending)
}

fn synchronous_failure_terminal(
    is_web_font: bool,
    web_font: Option<crate::css_resource_urls::StylesheetWebFont>,
) -> StylesheetSubresourceFetchStart {
    if is_web_font {
        StylesheetSubresourceFetchStart::WebFontSettled(CompletedStylesheetWebFont::failure(
            web_font.expect("web-font metadata was validated before consumption"),
        ))
    } else {
        StylesheetSubresourceFetchStart::Settled
    }
}
