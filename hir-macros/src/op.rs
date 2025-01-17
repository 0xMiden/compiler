use proc_macro2::TokenStream;
use quote::quote;
use syn::{punctuated::Punctuated, DeriveInput, Token};

pub struct Op {
    generics: syn::Generics,
    ident: syn::Ident,
    fields: syn::Fields,
    args: OpArgs,
}

#[derive(Default)]
pub struct OpArgs {
    pub code: Option<Code>,
    pub severity: Option<Severity>,
    pub help: Option<Help>,
    pub labels: Option<Labels>,
    pub source_code: Option<SourceCode>,
    pub url: Option<Url>,
    pub forward: Option<Forward>,
    pub related: Option<Related>,
    pub diagnostic_source: Option<DiagnosticSource>,
}

pub enum OpArg {
    Transparent,
    Code(Code),
    Severity(Severity),
    Help(Help),
    Url(Url),
    Forward(Forward),
}

impl Parse for OpArg {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let ident = input.fork().parse::<syn::Ident>()?;
        if ident == "transparent" {
            // consume the token
            let _: syn::Ident = input.parse()?;
            Ok(OpArg::Transparent)
        } else if ident == "forward" {
            Ok(OpArg::Forward(input.parse()?))
        } else if ident == "code" {
            Ok(OpArg::Code(input.parse()?))
        } else if ident == "severity" {
            Ok(OpArg::Severity(input.parse()?))
        } else if ident == "help" {
            Ok(OpArg::Help(input.parse()?))
        } else if ident == "url" {
            Ok(OpArg::Url(input.parse()?))
        } else {
            Err(syn::Error::new(ident.span(), "Unrecognized diagnostic option"))
        }
    }
}

impl OpArgs {
    pub(crate) fn forward_or_override_enum(
        &self,
        variant: &syn::Ident,
        which_fn: WhichFn,
        mut f: impl FnMut(&ConcreteOpArgs) -> Option<TokenStream>,
    ) -> Option<TokenStream> {
        match self {
            Self::Transparent(forward) => Some(forward.gen_enum_match_arm(variant, which_fn)),
            Self::Concrete(concrete) => f(concrete).or_else(|| {
                concrete
                    .forward
                    .as_ref()
                    .map(|forward| forward.gen_enum_match_arm(variant, which_fn))
            }),
        }
    }
}

impl OpArgs {
    fn parse(
        _ident: &syn::Ident,
        fields: &syn::Fields,
        attrs: &[&syn::Attribute],
        allow_transparent: bool,
    ) -> syn::Result<Self> {
        let mut errors = Vec::new();

        let mut concrete = OpArgs::for_fields(fields)?;
        for attr in attrs {
            let args = attr.parse_args_with(Punctuated::<OpArg, Token![,]>::parse_terminated);
            let args = match args {
                Ok(args) => args,
                Err(error) => {
                    errors.push(error);
                    continue;
                }
            };

            concrete.add_args(attr, args, &mut errors);
        }

        let combined_error = errors.into_iter().reduce(|mut lhs, rhs| {
            lhs.combine(rhs);
            lhs
        });
        if let Some(error) = combined_error {
            Err(error)
        } else {
            Ok(concrete)
        }
    }

    fn for_fields(fields: &syn::Fields) -> Result<Self, syn::Error> {
        let labels = Labels::from_fields(fields)?;
        let source_code = SourceCode::from_fields(fields)?;
        let related = Related::from_fields(fields)?;
        let help = Help::from_fields(fields)?;
        let diagnostic_source = DiagnosticSource::from_fields(fields)?;
        Ok(Self {
            code: None,
            help,
            related,
            severity: None,
            labels,
            url: None,
            forward: None,
            source_code,
            diagnostic_source,
        })
    }

    fn add_args(
        &mut self,
        attr: &syn::Attribute,
        args: impl Iterator<Item = OpArg>,
        errors: &mut Vec<syn::Error>,
    ) {
        for arg in args {
            match arg {
                OpArg::Transparent => {
                    errors.push(syn::Error::new_spanned(attr, "transparent not allowed"));
                }
                OpArg::Forward(to_field) => {
                    if self.forward.is_some() {
                        errors.push(syn::Error::new_spanned(
                            attr,
                            "forward has already been specified",
                        ));
                    }
                    self.forward = Some(to_field);
                }
                OpArg::Code(new_code) => {
                    if self.code.is_some() {
                        errors
                            .push(syn::Error::new_spanned(attr, "code has already been specified"));
                    }
                    self.code = Some(new_code);
                }
                OpArg::Severity(sev) => {
                    if self.severity.is_some() {
                        errors.push(syn::Error::new_spanned(
                            attr,
                            "severity has already been specified",
                        ));
                    }
                    self.severity = Some(sev);
                }
                OpArg::Help(hl) => {
                    if self.help.is_some() {
                        errors
                            .push(syn::Error::new_spanned(attr, "help has already been specified"));
                    }
                    self.help = Some(hl);
                }
                OpArg::Url(u) => {
                    if self.url.is_some() {
                        errors
                            .push(syn::Error::new_spanned(attr, "url has already been specified"));
                    }
                    self.url = Some(u);
                }
            }
        }
    }
}

impl Op {
    pub fn from_derive_input(input: DeriveInput) -> Result<Self, syn::Error> {
        let input_attrs = input
            .attrs
            .iter()
            .filter(|x| x.path().is_ident("operation"))
            .collect::<Vec<&syn::Attribute>>();
        Ok(match input.data {
            syn::Data::Struct(data_struct) => {
                let args = OpArgs::parse(&input.ident, &data_struct.fields, &input_attrs, true)?;

                Op {
                    fields: data_struct.fields,
                    ident: input.ident,
                    generics: input.generics,
                    args,
                }
            }
            syn::Data::Enum(_) | syn::Data::Union(_) => {
                return Err(syn::Error::new(
                    input.ident.span(),
                    "Can't derive Op for enums or unions",
                ))
            }
        })
    }

    pub fn gen(&self) -> TokenStream {
        let (impl_generics, ty_generics, where_clause) = &self.generics.split_for_impl();
        let concrete = &self.args;
        let forward = |which| concrete.forward.as_ref().map(|fwd| fwd.gen_struct_method(which));
        let code_body = concrete
            .code
            .as_ref()
            .and_then(|x| x.gen_struct())
            .or_else(|| forward(WhichFn::Code));
        let help_body = concrete
            .help
            .as_ref()
            .and_then(|x| x.gen_struct(fields))
            .or_else(|| forward(WhichFn::Help));
        let sev_body = concrete
            .severity
            .as_ref()
            .and_then(|x| x.gen_struct())
            .or_else(|| forward(WhichFn::Severity));
        let rel_body = concrete
            .related
            .as_ref()
            .and_then(|x| x.gen_struct())
            .or_else(|| forward(WhichFn::Related));
        let url_body = concrete
            .url
            .as_ref()
            .and_then(|x| x.gen_struct(ident, fields))
            .or_else(|| forward(WhichFn::Url));
        let labels_body = concrete
            .labels
            .as_ref()
            .and_then(|x| x.gen_struct(fields))
            .or_else(|| forward(WhichFn::Labels));
        let src_body = concrete
            .source_code
            .as_ref()
            .and_then(|x| x.gen_struct(fields))
            .or_else(|| forward(WhichFn::SourceCode));
        let diagnostic_source = concrete
            .diagnostic_source
            .as_ref()
            .and_then(|x| x.gen_struct())
            .or_else(|| forward(WhichFn::DiagnosticSource));
        quote! {
            impl #impl_generics miette::Diagnostic for #ident #ty_generics #where_clause {
                #code_body
                #help_body
                #sev_body
                #rel_body
                #url_body
                #labels_body
                #src_body
                #diagnostic_source
            }
        }
    }
}
