//! Misc utils.

use proc_macro2::Span;
use syn::{spanned::Spanned, Attribute, FnArg, NestedMeta, Pat, PatType};

pub(crate) fn find_meta_attrs(
    name: &str,
    cr: Option<&str>,
    args: &[Attribute],
) -> Option<NestedMeta> {
    args.iter()
        .filter_map(|attr| attr.parse_meta().ok())
        .find(|meta| match_path(meta.path(), name, cr))
        .map(NestedMeta::from)
}

fn match_path(path: &syn::Path, name: &str, cr: Option<&str>) -> bool {
    if path.is_ident(name) {
        return true;
    } else if let Some(cr) = cr {
        if path.segments.len() == 2 {
            let crate_segment = &path.segments[0];
            let name_segment = &path.segments[1];
            return crate_segment.ident == cr && name_segment.ident == name;
        }
    }
    false
}

pub(crate) fn receiver_span(arg: &FnArg) -> Option<Span> {
    match arg {
        FnArg::Receiver(receiver) => Some(receiver.self_token.span()),
        FnArg::Typed(PatType { pat, .. }) => {
            if let Pat::Ident(pat_ident) = pat.as_ref() {
                if pat_ident.ident == "self" {
                    return Some(pat_ident.ident.span());
                }
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_paths() {
        let path: syn::Path = syn::parse_quote!(test);
        assert!(match_path(&path, "test", None));
        assert!(match_path(&path, "test", Some("crate")));
        assert!(!match_path(&path, "other", None));
        assert!(!match_path(&path, "other", Some("crate")));
        assert!(match_path(&path, "test", Some("crater")));

        let path: syn::Path = syn::parse_quote!(crate::test);
        assert!(!match_path(&path, "test", None));
        assert!(match_path(&path, "test", Some("crate")));
        assert!(!match_path(&path, "other", Some("crate")));
        assert!(!match_path(&path, "test", Some("crater")));
    }
}
