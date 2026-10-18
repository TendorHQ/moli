use quote::quote;
use syn::spanned::Spanned;
use syn::{Error, Field, Generics, Ident};

use crate::attrs::{RenameRule, field_member_name, parse_field_attrs};
use crate::converter::{
    ConverterKind, converter_kind, inner_type_for_field, is_option_type, vec_inner_type,
};

pub(crate) fn expand_snapshot(
    name: &Ident,
    generics: &Generics,
    fields: &[&Field],
    rename_all: RenameRule,
) -> Result<proc_macro2::TokenStream, Error> {
    let mut members = Vec::with_capacity(fields.len());
    for field in fields {
        let attrs = parse_field_attrs(field)?;
        if attrs.inherit || attrs.nullable || attrs.with.is_some() {
            return Err(Error::new(
                field.span(),
                "dictionary snapshots do not yet support inherit, nullable, or with",
            ));
        }
        let converter = converter_kind(field, &attrs)?;
        let mut ty = inner_type_for_field(field);
        if attrs.sequence {
            ty = vec_inner_type(ty).expect("validated sequence field");
        }
        let kind = match converter.kind {
            ConverterKind::Dictionary => quote! {
                ::moli_webidl::SnapshotKind::Dictionary(<#ty as ::moli_webidl::WebIdlDictionarySnapshot>::SNAPSHOT_MEMBERS)
            },
            ConverterKind::Raw => quote! {
                <#ty as ::moli_webidl::WebIdlSnapshotValue>::SNAPSHOT_KIND
            },
            ConverterKind::BufferSource
            | ConverterKind::CallbackFunction
            | ConverterKind::CallbackInterface => {
                return Err(Error::new(
                    field.span(),
                    "dictionary snapshots require an explicit raw policy for buffers and callbacks",
                ));
            }
            ConverterKind::DomString
            | ConverterKind::DomString16
            | ConverterKind::UsvString
            | ConverterKind::ByteString
            | ConverterKind::Octet
            | ConverterKind::Long
            | ConverterKind::LongLong
            | ConverterKind::EnforceRangeLong
            | ConverterKind::Short
            | ConverterKind::UnsignedShort
            | ConverterKind::EnforceRangeUnsignedShort
            | ConverterKind::ClampedUnsignedShort
            | ConverterKind::UnsignedLong
            | ConverterKind::EnforceRangeUnsignedLong
            | ConverterKind::EnforceRangeLongLong
            | ConverterKind::UnsignedLongLong
            | ConverterKind::EnforceRangeUnsignedLongLong
            | ConverterKind::Float
            | ConverterKind::UnrestrictedFloat
            | ConverterKind::Double
            | ConverterKind::UnrestrictedDouble
            | ConverterKind::Boolean
            | ConverterKind::Enum
            | ConverterKind::Interface => quote!(::moli_webidl::SnapshotKind::Identity),
        };
        let kind = if attrs.sequence {
            quote!(::moli_webidl::SnapshotKind::Sequence(&#kind))
        } else {
            kind
        };
        let member_name = field_member_name(field, &attrs, rename_all)?;
        let optional = is_option_type(&field.ty);
        members.push(quote! {
            ::moli_webidl::SnapshotMember { name: #member_name, kind: #kind, optional: #optional }
        });
    }
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    Ok(quote! {
        impl #impl_generics ::moli_webidl::WebIdlDictionarySnapshot for #name #ty_generics #where_clause {
            const SNAPSHOT_MEMBERS: &'static [::moli_webidl::SnapshotMember] = &[#(#members),*];
        }
    })
}
