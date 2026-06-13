// Copyright (c) The Diem Core Contributors
// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::sandbox::utils::on_disk_state_view::OnDiskStateView;
use anyhow::{Result, bail};
use move_bytecode_utils::layout::{SerdeLayoutBuilder, SerdeLayoutConfig};
use move_core_types::{
    identifier::Identifier,
    language_storage::{StructTag, TypeTag},
};
use serde_reflection::{ContainerFormat, Format, Named, Registry, VariantFormat};
use serde_yaml::{Mapping, Value};
use std::path::Path;

pub fn generate_struct_layouts(
    path: &Path,
    struct_opt: &Option<String>,
    type_params_opt: &Option<Vec<TypeTag>>,
    separator: Option<String>,
    omit_addresses: bool,
    ignore_phantom_types: bool,
    shallow: bool,
    state: &OnDiskStateView,
) -> Result<()> {
    if let Some(module_id) = state.get_module_id(path) {
        if let Some(struct_) = struct_opt {
            // Generate for one struct
            let type_params = type_params_opt.as_ref().cloned().unwrap_or_default();
            let name = Identifier::new(struct_.as_str())?;
            let struct_tag = StructTag {
                address: *module_id.address(),
                module: module_id.name().to_owned(),
                name,
                type_params,
            };
            let mut layout_builder = SerdeLayoutBuilder::new_with_config(
                &state,
                SerdeLayoutConfig {
                    separator,
                    omit_addresses,
                    ignore_phantom_types,
                    shallow,
                },
            );
            layout_builder.build_struct_layout(&struct_tag)?;
            let layout = serde_yaml::to_string(&registry_to_yaml(layout_builder.registry())?)?;
            state.save_struct_layouts(&layout)?;
            println!("{}", layout);
        } else {
            unimplemented!(
                "Generating layout for all structs in a module. Use the --module and --struct options"
            )
        }
        Ok(())
    } else {
        bail!("Can't resolve module at {:?}", path)
    }
}

fn tagged_value(tag: &str, value: Value) -> Value {
    let mut mapping = Mapping::new();
    mapping.insert(Value::String(tag.to_owned()), value);
    Value::Mapping(mapping)
}

fn named_value<T>(named: &Named<T>, convert: impl FnOnce(&T) -> Result<Value>) -> Result<Value> {
    let mut mapping = Mapping::new();
    mapping.insert(Value::String(named.name.clone()), convert(&named.value)?);
    Ok(Value::Mapping(mapping))
}

fn format_to_yaml(format: &Format) -> Result<Value> {
    let scalar = |name: &str| Ok(Value::String(name.to_owned()));
    match format {
        Format::Variable(_) => bail!("Cannot serialize unresolved format variables"),
        Format::TypeName(name) => Ok(tagged_value("TYPENAME", Value::String(name.clone()))),
        Format::Unit => scalar("UNIT"),
        Format::Bool => scalar("BOOL"),
        Format::I8 => scalar("I8"),
        Format::I16 => scalar("I16"),
        Format::I32 => scalar("I32"),
        Format::I64 => scalar("I64"),
        Format::I128 => scalar("I128"),
        Format::U8 => scalar("U8"),
        Format::U16 => scalar("U16"),
        Format::U32 => scalar("U32"),
        Format::U64 => scalar("U64"),
        Format::U128 => scalar("U128"),
        Format::F32 => scalar("F32"),
        Format::F64 => scalar("F64"),
        Format::Char => scalar("CHAR"),
        Format::Str => scalar("STR"),
        Format::Bytes => scalar("BYTES"),
        Format::Option(value) => Ok(tagged_value("OPTION", format_to_yaml(value)?)),
        Format::Seq(value) => Ok(tagged_value("SEQ", format_to_yaml(value)?)),
        Format::Map { key, value } => {
            let mut mapping = Mapping::new();
            mapping.insert(Value::String("KEY".to_owned()), format_to_yaml(key)?);
            mapping.insert(Value::String("VALUE".to_owned()), format_to_yaml(value)?);
            Ok(tagged_value("MAP", Value::Mapping(mapping)))
        }
        Format::Tuple(values) => Ok(tagged_value(
            "TUPLE",
            Value::Sequence(
                values
                    .iter()
                    .map(format_to_yaml)
                    .collect::<Result<Vec<_>>>()?,
            ),
        )),
        Format::TupleArray { content, size } => {
            let mut mapping = Mapping::new();
            mapping.insert(
                Value::String("CONTENT".to_owned()),
                format_to_yaml(content)?,
            );
            mapping.insert(
                Value::String("SIZE".to_owned()),
                serde_yaml::to_value(size)?,
            );
            Ok(tagged_value("TUPLEARRAY", Value::Mapping(mapping)))
        }
    }
}

fn variant_to_yaml(variant: &VariantFormat) -> Result<Value> {
    match variant {
        VariantFormat::Variable(_) => bail!("Cannot serialize unresolved variant variables"),
        VariantFormat::Unit => Ok(Value::String("UNIT".to_owned())),
        VariantFormat::NewType(value) => Ok(tagged_value("NEWTYPE", format_to_yaml(value)?)),
        VariantFormat::Tuple(values) => Ok(tagged_value(
            "TUPLE",
            Value::Sequence(
                values
                    .iter()
                    .map(format_to_yaml)
                    .collect::<Result<Vec<_>>>()?,
            ),
        )),
        VariantFormat::Struct(fields) => Ok(tagged_value(
            "STRUCT",
            Value::Sequence(
                fields
                    .iter()
                    .map(|field| named_value(field, format_to_yaml))
                    .collect::<Result<Vec<_>>>()?,
            ),
        )),
    }
}

fn container_to_yaml(container: &ContainerFormat) -> Result<Value> {
    match container {
        ContainerFormat::UnitStruct => Ok(Value::String("UNITSTRUCT".to_owned())),
        ContainerFormat::NewTypeStruct(value) => {
            Ok(tagged_value("NEWTYPESTRUCT", format_to_yaml(value)?))
        }
        ContainerFormat::TupleStruct(values) => Ok(tagged_value(
            "TUPLESTRUCT",
            Value::Sequence(
                values
                    .iter()
                    .map(format_to_yaml)
                    .collect::<Result<Vec<_>>>()?,
            ),
        )),
        ContainerFormat::Struct(fields) => Ok(tagged_value(
            "STRUCT",
            Value::Sequence(
                fields
                    .iter()
                    .map(|field| named_value(field, format_to_yaml))
                    .collect::<Result<Vec<_>>>()?,
            ),
        )),
        ContainerFormat::Enum(variants) => {
            let mut mapping = Mapping::new();
            for (index, variant) in variants {
                mapping.insert(
                    serde_yaml::to_value(index)?,
                    named_value(variant, variant_to_yaml)?,
                );
            }
            Ok(tagged_value("ENUM", Value::Mapping(mapping)))
        }
    }
}

fn registry_to_yaml(registry: &Registry) -> Result<Value> {
    let mut mapping = Mapping::new();
    for (name, container) in registry {
        mapping.insert(Value::String(name.clone()), container_to_yaml(container)?);
    }
    Ok(Value::Mapping(mapping))
}
