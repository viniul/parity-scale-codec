// Copyright 2017, 2018 Parity Technologies
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use proc_macro2::{Span, TokenStream, Ident};
use syn::{Data, Fields, Field, spanned::Spanned, Error};
use crate::utils;

pub fn quote(data: &Data, type_name: &Ident, input: &TokenStream) -> TokenStream {
	let call_site = Span::call_site();
	match *data {
		Data::Struct(ref data) => match data.fields {
			Fields::Named(_) | Fields::Unnamed(_) => create_instance(
				call_site,
				quote! { #type_name },
				input,
				&data.fields,
			),
			Fields::Unit => {
				quote_spanned! {call_site =>
					drop(#input);
					Ok(#type_name)
				}
			},
		},
		Data::Enum(ref data) => {
			let data_variants = || data.variants.iter().filter(|variant| crate::utils::get_skip(&variant.attrs).is_none());

			if data_variants().count() > 256 {
				return Error::new(
					Span::call_site(),
					"Currently only enums with at most 256 variants are encodable."
				).to_compile_error();
			}

			let recurse = data_variants().enumerate().map(|(i, v)| {
				let name = &v.ident;
				let index = utils::index(v, i);

				let create = create_instance(
					call_site,
					quote! { #type_name :: #name },
					input,
					&v.fields,
				);

				quote_spanned! { v.span() =>
					x if x == #index as u8 => {
						#create
					},
				}
			});

			let err_msg = format!("No such variant in enum {}", type_name);
			quote! {
				match #input.read_byte()? {
					#( #recurse )*
					x => Err(#err_msg.into()),
				}
			}

		},
		Data::Union(_) => Error::new(Span::call_site(), "Union types are not supported.").to_compile_error(),
	}
}

fn create_decode_expr(field: &Field, name: &String, input: &TokenStream) -> TokenStream {
	let encoded_as = utils::get_encoded_as_type(field);
	let compact = utils::get_enable_compact(field);
	let skip = utils::get_skip(&field.attrs).is_some();

	if encoded_as.is_some() as u8 + compact as u8 + skip as u8 > 1 {
		return Error::new(
			Span::call_site(),
			"`encoded_as`, `compact` and `skip` can only be used one at a time!"
		).to_compile_error();
	}

	let err_msg = format!("Error decoding field {}", name);

	if compact {
		let field_type = &field.ty;
		quote_spanned! { field.span() =>
			{
				let res = <
					<#field_type as _parity_scale_codec::HasCompact>::Type as _parity_scale_codec::Decode
				>::decode(#input);
				match res {
					Err(_) => return Err(#err_msg.into()),
					Ok(a) => a.into(),
				}
			}
		}
	} else if let Some(encoded_as) = encoded_as {
		quote_spanned! { field.span() =>
			{
				let res = <#encoded_as as _parity_scale_codec::Decode>::decode(#input);
				match res {
					Err(_) => return Err(#err_msg.into()),
					Ok(a) => a.into(),
				}
			}
		}
	} else if skip {
		quote_spanned! { field.span() => Default::default() }
	} else {
		quote_spanned! { field.span() =>
			{
				let res = _parity_scale_codec::Decode::decode(#input);
				match res {
					Err(_) => return Err(#err_msg.into()),
					Ok(a) => a,
				}
			}
		}
	}
}

fn create_instance(
	call_site: Span,
	name: TokenStream,
	input: &TokenStream,
	fields: &Fields
) -> TokenStream {
	match *fields {
		Fields::Named(ref fields) => {
			let recurse = fields.named.iter().map(|f| {
				let name_ident = &f.ident;
				let field = match name_ident {
					Some(a) => format!("{}.{}", name, a),
					None => format!("{}", name),
				};
				let decode = create_decode_expr(f, &field, input);

				quote_spanned! { f.span() =>
					#name_ident: #decode
				}
			});

			quote_spanned! {call_site =>
				Ok(#name {
					#( #recurse, )*
				})
			}
		},
		Fields::Unnamed(ref fields) => {
			let recurse = fields.unnamed.iter().enumerate().map(|(i, f) | {
				let name = format!("{}.{}", name, i);

				create_decode_expr(f, &name, input)
			});

			quote_spanned! {call_site =>
				Ok(#name (
					#( #recurse, )*
				))
			}
		},
		Fields::Unit => {
			quote_spanned! {call_site =>
				Ok(#name)
			}
		},
	}
}
