extern crate proc_macro;

mod attributes;
mod consts;

#[macro_use]
mod structures;
#[macro_use]
mod util;

use quote::{quote, ToTokens};
use syn::{parse_macro_input, spanned::Spanned, Lit};

use attributes::*;
use consts::*;
use structures::*;
use util::*;

#[proc_macro_attribute]
pub fn interaction_cmd(
    attr: proc_macro::TokenStream,
    input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let mut fun = parse_macro_input!(input as CommandFun);

    let _name = if !attr.is_empty() {
        parse_macro_input!(attr as Lit).to_str()
    } else {
        fun.name.to_string()
    };

    let mut options = InteractionOptions::new();

    for attribute in &fun.attributes {
        let span = attribute.span();
        let values = propagate_err!(parse_values(attribute));

        let name = values.name.to_string();
        let name = &name[..];

        match_options!(name, values, options, span => [
            checks;
            required_permissions;
            allowed_roles;
            owners_only;
            owner_privilege
        ]);
    }

    /* let InteractionOptions {
        checks,
        allowed_roles,
        required_permissions,
        owners_only,
        owner_privilege,
    } = options; */

    propagate_err!(create_declaration_validations(&mut fun, DeclarFor::Command));

    /* let options_path = quote!(super::interactions::InteractionOptions);
    let command_path = quote!(super::interactions::InteractionCmd); */

    /* let res = parse_quote!(super::interactions::InteractionResult);
    create_return_type_validation(&mut fun, res); */

    let visibility = fun.visibility;
    let name = fun.name.clone();
    /* let options = name.with_suffix(INTERACTION_OPTIONS); */
    let body = fun.body;
    /* let ret = fun.ret; */

    /* let n = name.with_suffix(INTERACTION); */

    let cooked = fun.cooked.clone();

    populate_fut_lifetimes_on_refs(&mut fun.args);
    let args = fun.args;

    /* let name_str = name.to_string(); */

    (quote! {
        /* #(#cooked)*
        #[allow(missing_docs)]
        pub static #options: #options_path = #options_path {
            checks: #checks,
            allowed_roles: &[#(#allowed_roles),*],
            required_permissions: #required_permissions,
            owners_only: #owners_only,
            owner_privilege: #owner_privilege,
        };

        #(#cooked)*
        #[allow(missing_docs)]
        pub static #n: #command_path = #command_path {
            name: #name_str,
            fun: #name,
            setup: setup,
            options: &#options,
        }; */

        #(#cooked)*
        #[allow(missing_docs)]
        #visibility fn #name<'fut> (#(#args),*) -> ::futures::future::BoxFuture<'fut, ::anyhow::Result<()>> {
            use ::futures::future::FutureExt;
            async move { #(#body)* }.boxed()
        }
    })
    .into()
}

#[proc_macro_attribute]
pub fn interaction_group(
    attr: proc_macro::TokenStream,
    input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let group = parse_macro_input!(input as GroupStruct);

    let name = if !attr.is_empty() {
        parse_macro_input!(attr as Lit).to_str()
    } else {
        group.name.to_string()
    };

    let mut options = GroupOptions::new();

    for attribute in &group.attributes {
        let span = attribute.span();
        let values = propagate_err!(parse_values(attribute));

        let name = values.name.to_string();
        let name = &name[..];

        match_options!(name, values, options, span => [
            owners_only;
            owner_privilege;
            allowed_roles;
            required_permissions;
            checks;
            default_command;
            commands;
            sub_groups
        ]);
    }

    let GroupOptions {
        owners_only,
        owner_privilege,
        allowed_roles,
        required_permissions,
        checks,
        default_command,
        commands,
        sub_groups,
    } = options;

    let cooked = group.cooked.clone();
    let n = group.name.with_suffix(INTERACTION_GROUP);

    let default_command = default_command.map(|ident| {
        let i = ident.with_suffix(INTERACTION);

        quote!(&#i)
    });

    let commands = commands
        .into_iter()
        .map(|c| c.with_suffix(INTERACTION))
        .collect::<Vec<_>>();

    let sub_groups = sub_groups
        .into_iter()
        .map(|c| c.with_suffix(INTERACTION_GROUP))
        .collect::<Vec<_>>();

    let options = group.name.with_suffix(INTERACTION_GROUP_OPTIONS);
    let options_path = quote!(interactions::InteractionGroupOptions);
    let group_path = quote!(interactions::InteractionGroup);

    (quote! {
        #(#cooked)*
        #[allow(missing_docs)]
        pub static #options: #options_path = #options_path {
            owners_only: #owners_only,
            owner_privilege: #owner_privilege,
            allowed_roles: &[#(#allowed_roles),*],
            required_permissions: #required_permissions,
            checks: #checks,
            default_command: #default_command,
            commands: &[#(&#commands),*],
            sub_groups: &[#(&#sub_groups),*],
        };

        #(#cooked)*
        #[allow(missing_docs)]
        pub static #n: #group_path = #group_path {
            name: #name,
            options: &#options,
        };

        #group
    })
    .into()
}

#[proc_macro]
pub fn interaction_setup(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let setup = parse_macro_input!(input as InteractionSetup);

    proc_macro::TokenStream::from(setup.into_token_stream())
}

#[proc_macro]
pub fn parse_interaction_options(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let params = parse_macro_input!(input as ParseInteractionOptions);

    let data = params.data;
    let options = params.options.iter();
    let declarations = params.options.iter().map(|o| o.declare_variable());

    let output = quote! {
        #(#declarations)*

        for option in &#data.options {
            if let Some(value) = &option.value {
                match option.name.as_str() {
                    #(#options)*

                    _ => ::log::error!(
                        "Unknown option '{}' found for command '{}'.",
                        option.name,
                        file!()
                    ),
                }
            }
        }

    };

    output.into()
}
