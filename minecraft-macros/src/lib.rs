use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Fields};

#[proc_macro_derive(PacketDeserializer)]
pub fn packet_deserializer_derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let struct_name = &input.ident;

    let fields = match input.data {
        Data::Struct(ref data) => match data.fields {
            Fields::Named(ref fields) => &fields.named,
            _ => panic!("Only named fields are supported"),
        },
        _ => panic!("Only structs are supported"),
    };

    let field_names: Vec<_> = fields.iter().map(|f| &f.ident).collect();
    let field_types: Vec<_> = fields.iter().map(|f| &f.ty).collect();

    let gen = quote! {
        impl PacketDeserializer for #struct_name {
            fn from_raw<RW: AsyncRead + AsyncWrite + Unpin>(stream: &mut MinecraftStream<RW>) -> Result<Self, ReadingError> {
                #(let #field_names = stream.read_field::<#field_types>()?;)*
                
                Ok(#struct_name {
                    #(#field_names),*
                })
            }
        }
    };

    return gen.into();
}

#[proc_macro_derive(PacketSerializer)]
pub fn packet_serializer_derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let struct_name = &input.ident;

    let fields = match input.data {
        Data::Struct(ref data) => match data.fields {
            Fields::Named(ref fields) => &fields.named,
            _ => panic!("Only named fields are supported"),
        },
        _ => panic!("Only structs are supported"),
    };

    let field_names: Vec<_> = fields.iter().map(|f| &f.ident).collect();
    let field_types: Vec<_> = fields.iter().map(|f| &f.ty).collect();

    let gen = quote! {
        impl PacketSerializer for #struct_name {
            fn to_raw(&self, stream: &mut Buffer) -> Option<()> {
                #(stream.write_field::<#field_types>(&self.#field_names)?;)*

                Some(())
            }
        }
    };

    return gen.into();
}
