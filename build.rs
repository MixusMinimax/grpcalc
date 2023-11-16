use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use proc_macro2::TokenStream;
use prost_build::{Config, Method, Service, ServiceGenerator};
use quote::ToTokens;

/// NOTE: modify this function
/// Imported messages are automatically included. Enable only the proto files that include the
/// SERVICES that are required for a project to function. This will generate server and client stubs.
fn proto_files() -> Vec<&'static str> {
    let mut protos = Vec::new();

    // protos.extend_from_slice(&[
    //     "src/main/proto/std/ScalarWrappers.proto",
    //     "src/main/proto/std/Time.proto",
    // ]);

    #[cfg(feature = "user")]
    protos.extend_from_slice(&["src/main/proto/user/user.proto"]);

    #[cfg(feature = "calculator")]
    protos.extend_from_slice(&["proto/calculator.proto"]);

    protos
}

//<editor-fold defaultstate="collapsed" desc="Implementation (do not edit) ...">

fn main() -> Result<(), Box<dyn std::error::Error>> {
    compile(
        &proto_files(),
        &["src/main/proto"], // used to resolve imports
        true,
        cfg!(feature = "generate-wrappers"),
    )?;
    generate_includes()?;
    Ok(())
}

fn compile(
    protos: &[impl AsRef<Path>],
    includes: &[impl AsRef<Path>],
    emit_rerun_if_changed: bool,
    generate_wrappers: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut config = Config::new();
    config
        .file_descriptor_set_path(
            PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("proto_descriptor.bin"),
        )
        .type_attribute(".", "#[derive(serde::Serialize, serde::Deserialize)]")
        .type_attribute(".", "#[serde(rename_all = \"camelCase\")]")
        .field_attribute("in", "#[serde(rename = \"in\")]");
    let internal_service_generator = tonic_build::configure()
        .generate_default_stubs(true)
        .service_generator();
    let service_generator = if generate_wrappers {
        Box::new(ServiceWrapperGenerator {
            internal_service_generator,
        })
    } else {
        internal_service_generator
    };
    config
        .out_dir(PathBuf::from(std::env::var("OUT_DIR").unwrap()))
        .service_generator(service_generator);

    if emit_rerun_if_changed {
        for path in protos {
            println!("cargo:rerun-if-changed={}", path.as_ref().display())
        }

        for path in includes {
            println!("cargo:rerun-if-changed={}", path.as_ref().display())
        }
    }

    config.compile_protos(protos, includes)?;

    Ok(())
}

struct ServiceWrapperGenerator {
    internal_service_generator: Box<dyn ServiceGenerator>,
}

impl ServiceGenerator for ServiceWrapperGenerator {
    fn generate(&mut self, service: Service, buf: &mut String) {
        let service_name = quote::format_ident!("{}", service.name);
        let wrapper_name = quote::format_ident!("{}Wrapper", service.name);
        let server_mod = quote::format_ident!("{}_server", naive_snake_case(&service.name));
        let wrapper_mod = quote::format_ident!("{}_wrapper", naive_snake_case(&service.name));

        fn is_google_type(ty: &str) -> bool {
            ty.starts_with(".google.protobuf")
        }

        const NON_PATH_TYPE_ALLOWLIST: &[&str] = &["()"];

        let request_response_name =
            |method: &Method, proto_path: &str, compile_well_known_types: bool| {
                let convert_type = |proto_type: &str, rust_type: &str| {
                    if (is_google_type(proto_type) && !compile_well_known_types)
                        || rust_type.starts_with("::")
                        || NON_PATH_TYPE_ALLOWLIST.iter().any(|ty| *ty == rust_type)
                    {
                        rust_type.parse().unwrap()
                    } else if rust_type.starts_with("crate::") {
                        syn::parse_str::<syn::Path>(rust_type)
                            .unwrap()
                            .to_token_stream()
                    } else {
                        syn::parse_str::<syn::Path>(&format!("{}::{}", proto_path, rust_type))
                            .unwrap()
                            .to_token_stream()
                    }
                };

                let request = convert_type(&method.input_proto_type, &method.input_type);
                let response = convert_type(&method.output_proto_type, &method.output_type);
                (request, response)
            };

        let method_delegates = service
            .methods
            .iter()
            .map(|method| {
                let method_name = quote::format_ident!("{}", method.name);
                let (input, output) = request_response_name(method, "super", true);
                quote::quote! {
                    async fn #method_name(
                        &self,
                        request: tonic::Request<#input>
                    ) -> std::result::Result<
                        tonic::Response<#output>,
                        tonic::Status,
                    > {
                        self.inner.#method_name(request).await
                    }
                }
            })
            .collect::<Vec<_>>();

        let service_wrapper = quote::quote! {
            pub mod #wrapper_mod {
                #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
                use tonic::codegen::*;

                pub struct #wrapper_name<T>
                where
                    T: ::std::ops::Deref<Target = dyn super::#server_mod::#service_name>,
                    T: Sync,
                    T: Send,
                    T: 'static,
                {
                    inner: T,
                }

                impl<T> #wrapper_name<T>
                where
                    T: ::std::ops::Deref<Target = dyn super::#server_mod::#service_name>,
                    T: Sync,
                    T: Send,
                    T: 'static,
                {
                    pub fn new(inner: T) -> Self {
                        Self { inner }
                    }
                }

                #[async_trait]
                impl<T> super::#server_mod::#service_name for #wrapper_name<T>
                where
                    T: ::std::ops::Deref<Target = dyn super::#server_mod::#service_name>,
                    T: Sync,
                    T: Send,
                    T: 'static,
                {
                    #(#method_delegates)*
                }
            }
        };

        let ast: syn::File = syn::parse2(service_wrapper).expect("not a valid token stream");
        let code = prettyplease::unparse(&ast);

        buf.push_str(&code);

        self.internal_service_generator.generate(service, buf);
    }

    fn finalize(&mut self, _buf: &mut String) {
        self.internal_service_generator.finalize(_buf);
    }
}

struct ModuleTree {
    name: String,
    file_name: Option<String>,
    children: HashMap<String, ModuleTree>,
}

/// generates the "__.rs" file that contains all relevant proto_include!() statements.
/// This generates these include statements for all proto packages that were generated by protoc,
/// meaning that it will include all proto packages that were imported by the proto files that were
/// passed to the protoc compiler.
/// If a message or service uses a message from another package, that package will be included.
fn generate_includes() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());

    let file_names = std::fs::read_dir(&out_dir)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.is_file() {
                Some(path)
            } else {
                None
            }
        })
        .filter(|path| path.extension().map(|ext| ext == "rs").unwrap_or(false))
        .filter(|path| path.file_name() != Some(OsStr::new("__.rs")))
        .filter_map(|path| path.file_stem().map(|stem| stem.to_owned()))
        .collect::<Vec<_>>();

    let mut modules = ModuleTree {
        name: "_".to_owned(),
        file_name: None,
        children: HashMap::new(),
    };

    for file_name in file_names.iter() {
        if file_name == "_" {
            modules.file_name = Some("_".to_owned());
            continue;
        }
        let module_path = file_name
            .to_str()
            .to_owned()
            .unwrap()
            .split('.')
            .collect::<Vec<_>>();
        let mut current = &mut modules;
        for &module in module_path.iter() {
            let child = current
                .children
                .entry(module.to_owned())
                .or_insert_with(|| ModuleTree {
                    name: module.to_owned(),
                    file_name: None,
                    children: HashMap::new(),
                });
            current = child
        }
        current.file_name = Some(file_name.to_str().unwrap().to_owned());
    }

    fn create_includes(module: ModuleTree) -> TokenStream {
        let include = module.file_name.map(|file_name| {
            quote::quote! {
                tonic::include_proto!(#file_name);
            }
        });
        let submodules = module
            .children
            .into_values()
            .map(|module| {
                let name_token = quote::format_ident!("{}", module.name);

                let content = create_includes(module);

                quote::quote! {
                    pub mod #name_token {
                        #content
                    }
                }
            })
            .collect::<Vec<_>>();
        quote::quote! {
            #include

            #(#submodules)*
        }
    }

    let includes = create_includes(modules);

    let ast: syn::File = syn::parse2(includes).expect("not a valid token stream");
    let code = prettyplease::unparse(&ast);

    std::fs::write(out_dir.join("__.rs"), code)?;

    Ok(())
}

/// Exact function used by tonic (copied).
fn naive_snake_case(name: &str) -> String {
    let mut s = String::new();
    let mut it = name.chars().peekable();

    while let Some(x) = it.next() {
        s.push(x.to_ascii_lowercase());
        if let Some(y) = it.peek() {
            if y.is_uppercase() {
                s.push('_');
            }
        }
    }

    s
}

//</editor-fold>
