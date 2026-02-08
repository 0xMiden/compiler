use darling::FromDeriveInput;
use inflector::Inflector;
use quote::{format_ident, quote, quote_spanned};
use syn::{Ident, parse_quote, parse_quote_spanned, spanned::Spanned};

/// Represents the parsed struct definition for the operation we wish to define
///
/// Only named structs are allowed at this time.
#[derive(Debug, FromDeriveInput)]
#[darling(
    attributes(attribute),
    forward_attrs(doc, cfg, allow, derive),
    supports(any)
)]
pub struct Attribute {
    ident: Ident,
    #[allow(unused)]
    vis: syn::Visibility,
    generics: syn::Generics,
    #[allow(unused)]
    attrs: Vec<syn::Attribute>,
    dialect: Ident,
    #[darling(default)]
    name: Option<Ident>,
    #[darling(default)]
    remote: Option<syn::Path>,
    #[darling(default)]
    default: Option<syn::Path>,
    #[darling(default)]
    bounds: Option<Vec<syn::WherePredicate>>,
    #[darling(default)]
    traits: darling::util::PathList,
    #[darling(default)]
    implements: darling::util::PathList,
}

impl Attribute {
    #[allow(unused)]
    pub fn value_type(&self) -> syn::Type {
        syn::Type::Path(syn::TypePath {
            qself: None,
            path: self.value_type_path(),
        })
    }

    pub fn value_type_path(&self) -> syn::Path {
        if let Some(remote) = self.remote.clone() {
            remote
        } else {
            syn::Path {
                leading_colon: None,
                segments: syn::punctuated::Punctuated::from_iter([syn::PathSegment {
                    ident: self.ident.clone(),
                    arguments: syn::PathArguments::None,
                }]),
            }
        }
    }
}

pub fn derive_attribute(input: &syn::DeriveInput) -> darling::Result<Attribute> {
    Attribute::from_derive_input(input)
}

impl quote::ToTokens for Attribute {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let value_type_name = self.value_type_path();
        let attr_name = self.name.clone().unwrap_or_else(|| {
            let name = self.ident.to_string().to_snake_case();
            format_ident!("{name}", span = self.ident.span())
        });
        let attr_name_base = &self.ident;
        let attr_struct_name = format_ident!("{attr_name_base}Attr", span = self.ident.span());

        let (impl_generics, ty_generics, where_clause) = self.generics.split_for_impl();

        let value_type_name_str = value_type_name.clone().into_token_stream().to_string();
        let struct_doc = syn::Lit::Str(syn::LitStr::new(
            &format!("An attribute which represents [{value_type_name_str}] values."),
            self.ident.span(),
        ));

        let mut debug_generics = self.generics.clone();
        for tp in debug_generics.type_params_mut() {
            tp.bounds.push(syn::TypeParamBound::Trait(syn::TraitBound {
                paren_token: None,
                modifier: syn::TraitBoundModifier::None,
                lifetimes: None,
                path: parse_quote!(::core::fmt::Debug),
            }));
        }
        let (debug_impl_generics, debug_ty_generics, debug_where_clause) =
            debug_generics.split_for_impl();

        let mut display_generics = self.generics.clone();
        for tp in display_generics.type_params_mut() {
            tp.bounds.push(syn::TypeParamBound::Trait(syn::TraitBound {
                paren_token: None,
                modifier: syn::TraitBoundModifier::None,
                lifetimes: None,
                path: parse_quote!(::core::fmt::Display),
            }));
        }
        let (display_impl_generics, display_ty_generics, display_where_clause) =
            display_generics.split_for_impl();

        let mut attr_impl_generics = self.generics.clone();
        if let Some(bounds) = self.bounds.as_ref() {
            let where_clause = attr_impl_generics.make_where_clause();
            where_clause.predicates.extend(bounds.iter().cloned());
        }
        let (attr_impl_generics, attr_ty_generics, attr_where_clause) =
            attr_impl_generics.split_for_impl();

        // struct $Attr
        tokens.extend(quote_spanned! { attr_name.span() =>
            #[doc = #struct_doc]
            ///
            /// It is not possible to construct values of this type directly, instead you must
            /// allocate them using a [Context](::midenc_hir::Context) via [`Self::create`].
            #[derive(Clone, PartialEq, Eq, Hash)]
            #[repr(C)]
            pub struct #attr_struct_name #ty_generics {
                _attr: ::midenc_hir::attributes::Attr,
                value: #value_type_name #ty_generics,
            }

            impl #impl_generics core::ops::Deref for #attr_struct_name #ty_generics #where_clause {
                type Target = #value_type_name #ty_generics;

                #[inline(always)]
                fn deref(&self) -> &Self::Target {
                    &self.value
                }
            }

            impl #impl_generics core::ops::DerefMut for #attr_struct_name #ty_generics #where_clause {
                #[inline(always)]
                fn deref_mut(&mut self) -> &mut Self::Target {
                    &mut self.value
                }
            }

            impl #debug_impl_generics ::core::fmt::Debug for #attr_struct_name #debug_ty_generics #debug_where_clause {
                fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                    f.debug_struct(stringify!(#attr_struct_name))
                        .field("attr", &self._attr)
                        .field("value", &self.value)
                        .finish()
                }
            }

            impl #display_impl_generics ::core::fmt::Display for #attr_struct_name #display_ty_generics #display_where_clause {
                fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                    use ::midenc_hir::ToCompactString;
                    let value = self.value.to_compact_string();
                    write!(f, "#{}<{value:?}>", self._attr.name())
                }
            }
        });

        // impl AsRef<T>/AsMut<T>
        // impl AsRef<dyn Attr>/AsMut<dyn Attr>
        tokens.extend(quote_spanned! { attr_name.span() =>
            impl #impl_generics AsRef<#value_type_name #ty_generics> for #attr_struct_name #ty_generics #where_clause {
                #[inline(always)]
                fn as_ref(&self) -> &#value_type_name #ty_generics {
                    &self.value
                }
            }

            impl #impl_generics AsMut<#value_type_name #ty_generics> for #attr_struct_name #ty_generics #where_clause {
                #[inline(always)]
                fn as_mut(&mut self) -> &mut #value_type_name #ty_generics {
                    &mut self.value
                }
            }

            impl #impl_generics AsRef<::midenc_hir::attributes::Attr> for #attr_struct_name #ty_generics #where_clause {
                #[inline(always)]
                fn as_ref(&self) -> &::midenc_hir::attributes::Attr {
                    &self._attr
                }
            }

            impl #impl_generics AsMut<::midenc_hir::attributes::Attr> for #attr_struct_name #ty_generics #where_clause {
                #[inline(always)]
                fn as_mut(&mut self) -> &mut ::midenc_hir::attributes::Attr {
                    &mut self._attr
                }
            }
        });

        // impl Attribute
        // impl AttributeRegistration

        let create_default_impl: syn::ImplItemFn = if let Some(default_ctor) = self.default.as_ref()
        {
            parse_quote_spanned! { default_ctor.span() =>
                fn create_default(context: &::alloc::rc::Rc<::midenc_hir::Context>) -> ::midenc_hir::UnsafeIntrusiveEntityRef<Self> {
                    let value: <Self as ::midenc_hir::AttributeRegistration>::Value = #default_ctor();
                    let ty = <Self as ::midenc_hir::attributes::MaybeInferAttributeType>::maybe_infer_type_from_value(&value);
                    Self::create(context, value, ty.unwrap_or(::midenc_hir::Type::Unknown))
                }
            }
        } else {
            parse_quote_spanned! { attr_name.span() =>
                fn create_default(context: &::alloc::rc::Rc<::midenc_hir::Context>) -> ::midenc_hir::UnsafeIntrusiveEntityRef<Self> {
                    let value = <<Self as ::midenc_hir::AttributeRegistration>::Value>::default();
                    let ty = <Self as ::midenc_hir::attributes::MaybeInferAttributeType>::maybe_infer_type_from_value(&value);
                    Self::create(context, value, ty.unwrap_or(::midenc_hir::Type::Unknown))
                }
            }
        };

        let dialect = &self.dialect;
        let attr_name_str =
            syn::Lit::Str(syn::LitStr::new(&attr_name.to_string(), attr_name.span()));
        let traits = &self.traits;
        let implements = &self.implements;
        tokens.extend(quote! {
            impl #attr_impl_generics ::midenc_hir::Attribute for #attr_struct_name #attr_ty_generics #attr_where_clause {
                fn context(&self) -> &::midenc_hir::Context {
                    self._attr.context()
                }
                fn context_rc(&self) -> ::alloc::rc::Rc<::midenc_hir::Context> {
                    self._attr.context_rc()
                }
                #[inline]
                fn name(&self) -> &::midenc_hir::AttributeName {
                    self._attr.name()
                }

                #[inline(always)]
                fn as_attr(&self) -> &::midenc_hir::attributes::Attr {
                    &self._attr
                }

                #[inline(always)]
                fn as_attr_mut(&mut self) -> &mut ::midenc_hir::attributes::Attr {
                    &mut self._attr
                }
                #[inline(always)]
                fn value(&self) -> &dyn ::midenc_hir::attributes::AttributeValue {
                    &self.value
                }
                #[inline(always)]
                fn value_mut(&mut self) -> &mut dyn ::midenc_hir::attributes::AttributeValue {
                    &mut self.value
                }
                fn ty(&self) -> &::midenc_hir::Type {
                    self._attr.ty()
                }
                fn set_type(&mut self, ty: ::midenc_hir::Type) {
                    self._attr.set_type(ty);
                }
            }

            impl #attr_impl_generics #attr_struct_name #attr_ty_generics #attr_where_clause {
                #[inline]
                pub const fn as_value(&self) -> &#value_type_name #ty_generics {
                    &self.value
                }

                #[inline]
                pub fn as_value_mut(&mut self) -> &mut #value_type_name #ty_generics {
                    &mut self.value
                }
            }

            impl #attr_impl_generics ::midenc_hir::AttributeRegistration for #attr_struct_name #attr_ty_generics #attr_where_clause {
                type Value = #value_type_name #ty_generics;

                fn dialect_name() -> ::midenc_hir::interner::Symbol {
                    let namespace = <#dialect as ::midenc_hir::DialectRegistration>::NAMESPACE;
                    ::midenc_hir::interner::Symbol::intern(namespace)
                }

                fn name() -> ::midenc_hir::interner::Symbol {
                    ::midenc_hir::interner::Symbol::intern(#attr_name_str)
                }

                fn traits() -> ::alloc::boxed::Box<[::midenc_hir::traits::TraitInfo]> {
                    ::alloc::boxed::Box::from([
                        ::midenc_hir::traits::TraitInfo::new::<Self, dyn core::any::Any>(),
                        ::midenc_hir::traits::TraitInfo::new::<Self, dyn ::midenc_hir::Attribute>(),
                        #(
                            ::midenc_hir::traits::TraitInfo::new::<Self, dyn #traits>(),
                        )*
                        #(
                            ::midenc_hir::traits::TraitInfo::new::<Self, dyn #implements>(),
                        )*
                    ])
                }

                fn create<AttrValue>(
                    context: &::alloc::rc::Rc<::midenc_hir::Context>,
                    value: AttrValue,
                    ty: ::midenc_hir::Type
                ) -> ::midenc_hir::UnsafeIntrusiveEntityRef<Self>
                where
                    #value_type_name #ty_generics: From<AttrValue>,
                {
                    let name = context.get_registered_attribute_name::<#attr_struct_name #ty_generics>();
                    let value = <#value_type_name #ty_generics>::from(value);
                    unsafe {
                        let offset = ::core::mem::offset_of!(#attr_struct_name #ty_generics, _attr);
                        let _attr = ::midenc_hir::attributes::Attr::uninit::<Self>(context, name, ty, offset);
                        context.alloc_tracked(Self {
                            _attr,
                            value,
                        })
                    }
                }

                #create_default_impl

                #[inline(always)]
                fn underlying_value(attr: &Self) -> &Self::Value {
                    &attr.value
                }

                #[inline(always)]
                fn underlying_value_mut(attr: &mut Self) -> &mut Self::Value {
                    &mut attr.value
                }
            }
        });

        // impl $DerivedTrait
        for derived_trait in self.traits.iter() {
            tokens.extend(quote_spanned! { derived_trait.span() =>
                impl #attr_impl_generics #derived_trait for #attr_struct_name #attr_ty_generics #attr_where_clause {}
            });
        }
    }
}
