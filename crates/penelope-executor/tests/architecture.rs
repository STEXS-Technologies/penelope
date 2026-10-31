//! Structural library and fuzz-contract checks, using Rust syntax trees.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};
use syn::{
    Expr, GenericArgument, Item, Type,
    punctuated::Punctuated,
    visit::{self, Visit},
};

fn rust_files(directory: &Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            result.extend(rust_files(&path));
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            result.push(path);
        }
    }
    result
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn declaration_only(file: &syn::File) -> bool {
    file.items.iter().all(|item| {
        matches!(item, Item::Use(_))
            || matches!(item, Item::Mod(module) if module.content.is_none())
    })
}

#[test]
fn module_entry_files_contain_only_declarations_and_exports() {
    for file in rust_files(&root().join("crates")) {
        if matches!(
            file.file_name().and_then(|name| name.to_str()),
            Some("lib.rs" | "mod.rs")
        ) {
            let ast = syn::parse_file(&fs::read_to_string(&file).unwrap()).unwrap();
            assert!(
                declaration_only(&ast),
                "implementation in {}",
                file.display()
            );
        }
    }
}

#[test]
fn module_rule_rejects_implementations_and_inline_modules() {
    for source in [
        "fn execute() {}",
        "pub struct Id(u32);",
        "mod inline { pub fn execute() {} }",
        "include!(\"implementation.rs\");",
    ] {
        assert!(!declaration_only(&syn::parse_file(source).unwrap()));
    }
    assert!(declaration_only(
        &syn::parse_file(
            "//! Module documentation\nmod implementation; pub use implementation::*;"
        )
        .unwrap()
    ));
}

fn collect_newtypes(items: &[Item], result: &mut BTreeSet<String>) {
    for item in items {
        match item {
            Item::Struct(item) if matches!(item.vis, syn::Visibility::Public(_)) => {
                // Generic adapter/service wrappers are exercised by their own
                // lifecycle fuzz targets, rather than wire deserialization.
                if item.fields.len() == 1 && item.generics.params.is_empty() {
                    result.insert(item.ident.to_string());
                }
            }
            Item::Macro(item)
                if item.mac.path.is_ident("identifier")
                    || item.mac.path.is_ident("digest_identity")
                    || item.mac.path.is_ident("counter") =>
            {
                let arguments = item
                    .mac
                    .parse_body_with(Punctuated::<Expr, syn::Token![,]>::parse_terminated)
                    .unwrap();
                let Some(Expr::Path(first)) = arguments.first() else {
                    panic!("newtype macro must name its type");
                };
                result.insert(first.path.get_ident().unwrap().to_string());
            }
            Item::Mod(module) => {
                let test_only = module.attrs.iter().any(|attribute| {
                    attribute.path().is_ident("cfg")
                        && attribute
                            .parse_args::<syn::Path>()
                            .is_ok_and(|path| path.is_ident("test"))
                });
                if let (false, Some((_, nested_items))) = (test_only, &module.content) {
                    collect_newtypes(nested_items, result);
                }
            }
            Item::Macro(item) if item.mac.path.is_ident("thread_local") => {
                // Standard TLS declares statics rather than wire types. Parse
                // the body instead of silently accepting arbitrary macro items.
                let declarations = syn::parse2::<syn::File>(item.mac.tokens.clone()).unwrap();
                assert!(
                    declarations
                        .items
                        .iter()
                        .all(|declaration| matches!(declaration, Item::Static(_))),
                    "thread_local must contain only static declarations"
                );
            }
            Item::Macro(item)
                if item.mac.path.is_ident("canonical_wire_impl")
                    || item.mac.path.is_ident("canonical_port_impl") => {}
            Item::Macro(item) if item.ident.is_none() => panic!(
                "register public-type inventory support for macro {}",
                item.mac.path.segments.last().unwrap().ident
            ),
            Item::Const(_)
            | Item::Enum(_)
            | Item::ExternCrate(_)
            | Item::Fn(_)
            | Item::ForeignMod(_)
            | Item::Impl(_)
            | Item::Macro(_)
            | Item::Static(_)
            | Item::Struct(_)
            | Item::Trait(_)
            | Item::TraitAlias(_)
            | Item::Type(_)
            | Item::Union(_)
            | Item::Use(_)
            | Item::Verbatim(_) => {}
            _ => panic!("register public-type inventory support for new Rust item"),
        }
    }
}

#[derive(Default)]
struct FuzzedNewtypes(BTreeSet<String>);

impl FuzzedNewtypes {
    fn add(&mut self, ty: &Type) {
        if let Type::Path(path) = ty {
            self.0
                .insert(path.path.segments.last().unwrap().ident.to_string());
        }
    }
}

impl<'ast> Visit<'ast> for FuzzedNewtypes {
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let Expr::Path(function) = call.func.as_ref() {
            let segment = function.path.segments.last().unwrap();
            if let (true, syn::PathArguments::AngleBracketed(arguments)) = (
                segment.ident == "wire" || segment.ident == "identifier",
                &segment.arguments,
            ) {
                for argument in &arguments.args {
                    if let GenericArgument::Type(ty) = argument {
                        self.add(ty);
                    }
                }
            }
        }
        visit::visit_expr_call(self, call);
    }

    fn visit_macro(&mut self, invocation: &'ast syn::Macro) {
        if invocation.path.is_ident("wires") {
            let types = invocation
                .parse_body_with(Punctuated::<Type, syn::Token![,]>::parse_terminated)
                .unwrap();
            for ty in types {
                self.add(&ty);
            }
        }
        visit::visit_macro(self, invocation);
    }
}

#[test]
fn fuzz_inventory_counts_executed_calls_and_ignores_imports() {
    let ast = syn::parse_file("use library::OnlyImported; fn target() { identifier::<First>(b\"x\", \"id_\"); wires!(Second, Third); }").unwrap();
    let mut covered = FuzzedNewtypes::default();
    covered.visit_file(&ast);
    assert_eq!(
        covered.0,
        BTreeSet::from(["First".into(), "Second".into(), "Third".into()])
    );
    let mut required = BTreeSet::new();
    collect_newtypes(
        &syn::parse_file("pub struct Fourth(u32); identifier!(Fifth, \"id_\", Error, \"Id\"); pub struct Sixth { value: u64 } pub struct Service<J> { journal: J }")
            .unwrap()
            .items,
        &mut required,
    );
    assert_eq!(
        required,
        BTreeSet::from(["Fourth".into(), "Fifth".into(), "Sixth".into()])
    );
    assert_eq!(required.difference(&covered.0).count(), 3);
}

#[test]
fn every_public_newtype_has_an_explicit_fuzz_invocation() {
    let mut required = BTreeSet::new();
    for crate_entry in fs::read_dir(root().join("crates")).unwrap() {
        for file in rust_files(&crate_entry.unwrap().path().join("src")) {
            let ast = syn::parse_file(&fs::read_to_string(file).unwrap()).unwrap();
            collect_newtypes(&ast.items, &mut required);
        }
    }
    let source = fs::read_to_string(root().join("fuzz/fuzz_targets/fuzz_newtypes.rs")).unwrap();
    let ast = syn::parse_file(&source).unwrap();
    let mut covered = FuzzedNewtypes::default();
    for item in ast.items {
        let Item::Macro(item) = item else {
            continue;
        };
        if !item.mac.path.is_ident("fuzz_target") {
            continue;
        }
        let closure: syn::ExprClosure = item.mac.parse_body().unwrap();
        covered.visit_expr_closure(&closure);
    }
    let missing: Vec<_> = required.difference(&covered.0).collect();
    assert!(
        missing.is_empty(),
        "public newtypes without direct fuzz invocation: {missing:?}"
    );
    assert!(!required.is_empty());
}

#[test]
fn public_type_inventory_accepts_only_static_thread_local_declarations() {
    let valid = syn::parse_file(
        "thread_local! { static VALUE: std::cell::Cell<u8> = const { std::cell::Cell::new(0) }; }",
    )
    .unwrap();
    let mut types = BTreeSet::new();
    collect_newtypes(&valid.items, &mut types);
    assert!(types.is_empty());
    let invalid = syn::parse_file("thread_local! { pub struct Hidden(u8); }").unwrap();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            collect_newtypes(&invalid.items, &mut types);
        }))
        .is_err()
    );
}
