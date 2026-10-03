// Copyright (c) 2026 Jaroslav Pachola
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

//! A minimal reader for the kernel's BTF type information, enough to compute
//! struct member offsets. The format is described in the kernel's
//! Documentation/bpf/btf.rst; this reads only native-endian BTF, which is what
//! /sys/kernel/btf/vmlinux always is.

use std::path::Path;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;

const MAGIC: u16 = 0xeb9f;

// BTF_KIND_* from include/uapi/linux/btf.h
const KIND_INT: u32 = 1;
const KIND_PTR: u32 = 2;
const KIND_ARRAY: u32 = 3;
const KIND_STRUCT: u32 = 4;
const KIND_UNION: u32 = 5;
const KIND_ENUM: u32 = 6;
const KIND_FWD: u32 = 7;
const KIND_TYPEDEF: u32 = 8;
const KIND_VOLATILE: u32 = 9;
const KIND_CONST: u32 = 10;
const KIND_RESTRICT: u32 = 11;
const KIND_FUNC: u32 = 12;
const KIND_FUNC_PROTO: u32 = 13;
const KIND_VAR: u32 = 14;
const KIND_DATASEC: u32 = 15;
const KIND_FLOAT: u32 = 16;
const KIND_DECL_TAG: u32 = 17;
const KIND_TYPE_TAG: u32 = 18;
const KIND_ENUM64: u32 = 19;

/// A BTF type ID. 0 is void.
pub type TypeId = u32;

pub struct Member {
    pub name: String,
    pub type_id: TypeId,
    pub bit_offset: u32,
    /// 0 unless the member is a bitfield.
    pub bitfield_size: u32,
}

pub struct Enumerator {
    pub name: String,
    pub value: i32,
}

/// The kinds of type member offsets are computed through. Everything else
/// (functions, variables, sections, floats, ...) is `Other`.
pub enum Kind {
    Void,
    Int {
        size: u32,
    },
    Ptr,
    Array {
        element: TypeId,
        len: u32,
    },
    Struct {
        size: u32,
        members: Vec<Member>,
    },
    Union {
        size: u32,
        members: Vec<Member>,
    },
    Enum {
        size: u32,
        values: Vec<Enumerator>,
    },
    Enum64 {
        size: u32,
    },
    Fwd,
    /// Typedefs and the const, volatile, restrict and type tag qualifiers,
    /// which all just refer to another type.
    Alias(TypeId),
    Other,
}

pub struct Type {
    pub name: String,
    pub kind: Kind,
}

pub struct Btf {
    types: Vec<Type>,
}

/// Reads native-endian integers off the front of a byte slice.
struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn u32(&mut self) -> Result<u32> {
        let (bytes, rest) = self
            .0
            .split_first_chunk::<4>()
            .ok_or_else(|| anyhow!("BTF type section is truncated"))?;
        self.0 = rest;
        Ok(u32::from_ne_bytes(*bytes))
    }
}

impl Btf {
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let data =
            std::fs::read(path).with_context(|| format!("Failed to read {}", path.display()))?;
        Self::parse(&data).with_context(|| format!("Failed to parse BTF in {}", path.display()))
    }

    pub fn parse(data: &[u8]) -> Result<Self> {
        let field = |offset: usize, len: usize| {
            data.get(offset..offset + len)
                .ok_or_else(|| anyhow!("BTF header is truncated"))
        };
        let u32_at = |offset| field(offset, 4).map(|b| u32::from_ne_bytes(b.try_into().unwrap()));

        let magic = u16::from_ne_bytes(field(0, 2)?.try_into().unwrap());
        if magic != MAGIC {
            bail!("Bad BTF magic {magic:#x}");
        }
        let hdr_len = u32_at(4)? as usize;
        let section = |offset: usize, len: usize| {
            data.get(hdr_len + offset..hdr_len + offset + len)
                .ok_or_else(|| anyhow!("BTF section is out of bounds"))
        };
        let type_section = section(u32_at(8)? as usize, u32_at(12)? as usize)?;
        let strings = section(u32_at(16)? as usize, u32_at(20)? as usize)?;
        let name = |offset: u32| -> Result<String> {
            let tail = strings
                .get(offset as usize..)
                .ok_or_else(|| anyhow!("BTF string offset {offset} is out of bounds"))?;
            let end = tail.iter().position(|&b| b == 0).unwrap_or(tail.len());
            Ok(String::from_utf8_lossy(&tail[..end]).into_owned())
        };

        let mut types = vec![Type {
            name: String::new(),
            kind: Kind::Void,
        }];
        let mut r = Reader(type_section);
        while !r.0.is_empty() {
            let name_off = r.u32()?;
            let info = r.u32()?;
            let size_or_type = r.u32()?;
            let vlen = info & 0xffff;
            let kind_flag = info >> 31 != 0;
            let kind = match (info >> 24) & 0x1f {
                KIND_INT => {
                    r.u32()?;
                    Kind::Int { size: size_or_type }
                }
                KIND_PTR => Kind::Ptr,
                KIND_ARRAY => {
                    let element = r.u32()?;
                    let _index_type = r.u32()?;
                    let len = r.u32()?;
                    Kind::Array { element, len }
                }
                k @ (KIND_STRUCT | KIND_UNION) => {
                    let mut members = Vec::with_capacity(vlen as usize);
                    for _ in 0..vlen {
                        let name = name(r.u32()?)?;
                        let type_id = r.u32()?;
                        let offset = r.u32()?;
                        let (bit_offset, bitfield_size) = if kind_flag {
                            (offset & 0xff_ffff, offset >> 24)
                        } else {
                            (offset, 0)
                        };
                        members.push(Member {
                            name,
                            type_id,
                            bit_offset,
                            bitfield_size,
                        });
                    }
                    let size = size_or_type;
                    if k == KIND_STRUCT {
                        Kind::Struct { size, members }
                    } else {
                        Kind::Union { size, members }
                    }
                }
                KIND_ENUM => {
                    let mut values = Vec::with_capacity(vlen as usize);
                    for _ in 0..vlen {
                        let name = name(r.u32()?)?;
                        let value = r.u32()? as i32;
                        values.push(Enumerator { name, value });
                    }
                    Kind::Enum {
                        size: size_or_type,
                        values,
                    }
                }
                KIND_ENUM64 => {
                    for _ in 0..vlen * 3 {
                        r.u32()?;
                    }
                    Kind::Enum64 { size: size_or_type }
                }
                KIND_FWD => Kind::Fwd,
                KIND_TYPEDEF | KIND_VOLATILE | KIND_CONST | KIND_RESTRICT | KIND_TYPE_TAG => {
                    Kind::Alias(size_or_type)
                }
                KIND_FUNC | KIND_FLOAT => Kind::Other,
                KIND_FUNC_PROTO => {
                    for _ in 0..vlen * 2 {
                        r.u32()?;
                    }
                    Kind::Other
                }
                KIND_VAR | KIND_DECL_TAG => {
                    r.u32()?;
                    Kind::Other
                }
                KIND_DATASEC => {
                    for _ in 0..vlen * 3 {
                        r.u32()?;
                    }
                    Kind::Other
                }
                other => bail!("Unknown BTF kind {other} for type {}", types.len()),
            };
            types.push(Type {
                name: name(name_off)?,
                kind,
            });
        }
        Ok(Self { types })
    }

    pub fn type_by_id(&self, id: TypeId) -> Result<&Type> {
        self.types
            .get(id as usize)
            .ok_or_else(|| anyhow!("BTF type {id} does not exist"))
    }

    /// Every type, with its ID.
    pub fn types(&self) -> impl Iterator<Item = (TypeId, &Type)> {
        (0..).zip(&self.types)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Assembles BTF from type records (as u32 words) and a string section.
    fn build(types: &[u32], strings: &[u8]) -> Vec<u8> {
        let type_len = (types.len() * 4) as u32;
        let mut out = Vec::new();
        out.extend_from_slice(&MAGIC.to_ne_bytes());
        out.extend_from_slice(&[1, 0]); // version, flags
        for word in [24, 0, type_len, type_len, strings.len() as u32] {
            out.extend_from_slice(&u32::to_ne_bytes(word));
        }
        for word in types {
            out.extend_from_slice(&word.to_ne_bytes());
        }
        out.extend_from_slice(strings);
        out
    }

    #[test]
    fn parses_struct_with_bitfield_and_alias() {
        let strings = b"\0int\0s\0a\0b\0t\0";
        #[rustfmt::skip]
        let types = [
            // 1: int, 4 bytes
            1, KIND_INT << 24, 4, 32,
            // 2: struct s { int a; int b:3; }, kind_flag set
            5, (1 << 31) | (KIND_STRUCT << 24) | 2, 8,
            7, 1, 0,
            9, 1, (3 << 24) | 32,
            // 3: typedef struct s t
            11, KIND_TYPEDEF << 24, 2,
            // 4: a function prototype with one parameter, skipped
            0, (KIND_FUNC_PROTO << 24) | 1, 1,
            0, 1,
            // 5: int[7]
            0, KIND_ARRAY << 24, 0,
            1, 1, 7,
        ];
        let btf = Btf::parse(&build(&types, strings)).unwrap();

        let Kind::Struct { size, members } = &btf.type_by_id(2).unwrap().kind else {
            panic!("type 2 is not a struct");
        };
        assert_eq!(*size, 8);
        assert_eq!(members[0].name, "a");
        assert_eq!((members[0].bit_offset, members[0].bitfield_size), (0, 0));
        assert_eq!(members[1].name, "b");
        assert_eq!((members[1].bit_offset, members[1].bitfield_size), (32, 3));

        let t = btf.type_by_id(3).unwrap();
        assert_eq!(t.name, "t");
        assert!(matches!(t.kind, Kind::Alias(2)));
        assert!(matches!(btf.type_by_id(4).unwrap().kind, Kind::Other));
        assert!(matches!(
            btf.type_by_id(5).unwrap().kind,
            Kind::Array { element: 1, len: 7 }
        ));
        assert!(btf.type_by_id(6).is_err());
    }

    #[test]
    fn rejects_bad_input() {
        assert!(Btf::parse(&[0; 4]).is_err());
        let mut truncated = build(&[1, KIND_INT << 24, 4, 32], b"\0int\0");
        // A type_len of 8 cuts the 16-byte int record short.
        truncated[12..16].copy_from_slice(&8u32.to_ne_bytes());
        assert!(Btf::parse(&truncated).is_err());
    }
}
