// Copyright (C) 2026 0xtengu
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 2 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

//! Container parsing: ELF, PE, COFF/BOF and friends through one `object` trait surface.

use crate::score::Metrics;
use object::{
    ExportTarget, NameOrOrdinal, Object, ObjectSection, ObjectSymbol, SectionFlags, SymbolKind,
};
use std::collections::{HashMap, HashSet};

#[derive(Clone)]
pub struct Section
{
    pub name: String,
    pub offset: u64,
    pub size: u64,
    pub metrics: Metrics,
    pub executable: bool,
    pub writable: bool,
    pub symbols: usize,
    pub functions: usize,
    /// Section held no file bytes (ELF NOBITS / .bss). Its entropy is undefined.
    pub nobits: bool,
}

#[derive(Clone, Debug)]
pub struct Import
{
    pub library: String,
    pub name: String,
    pub ordinal: Option<u16>,
    pub weak: bool,
}

#[derive(Clone, Debug)]
pub struct Export
{
    pub name: String,
    pub ordinal: Option<u16>,
    pub address: Option<u64>,
    pub reexport: bool,
    pub weak: bool,
}

#[derive(Clone, Debug)]
pub struct Symbol
{
    pub name: String,
    pub address: u64,
    pub size: u64,
    pub kind: String,
    pub section: Option<String>,
    pub global: bool,
}

/// A function identified by object-file symbol metadata, not disassembly.
#[derive(Clone, Debug)]
pub struct Function
{
    pub name: String,
    pub address: u64,
    pub size: u64,
    pub file_offset: Option<u64>,
    pub section: Option<String>,
    pub global: bool,
}

/// What the file appears to be by magic number, independent of whether sections can be parsed.
pub struct Identity
{
    pub label: String,
    /// Short tag for table columns: "gzip", "elf", "png", "text".
    pub short: String,
    pub mime: Option<String>,
    /// Already-compressed container: sweeping codecs over it is near-pointless.
    pub precompressed: bool,
}

pub fn identify(data: &[u8]) -> Identity
{
    if let Some(t) = infer::get(data)
    {
        let mime = t.mime_type().to_string();
        let precompressed = matches!(
            t.matcher_type(),
            infer::MatcherType::Archive
                | infer::MatcherType::Image
                | infer::MatcherType::Video
                | infer::MatcherType::Audio
        ) && !matches!(t.extension(), "tar" | "ar" | "cpio");
        return Identity {
            label: format!("{} ({})", t.extension(), mime),
            short: t.extension().to_string(),
            mime: Some(mime),
            precompressed,
        };
    }
    let label = if data.len() >= 8 && &data[0..8] == b"!<arch>\n"
    {
        "ar archive"
    }
    else if data.len() >= 4 && &data[0..4] == b"\x7fELF"
    {
        "ELF"
    }
    else if data.len() >= 2 && &data[0..2] == b"MZ"
    {
        "PE/DOS"
    }
    else if data.len() >= 4 && &data[0..4] == b"dex\n"
    {
        "Android dex"
    }
    else if data.len() >= 4 && (&data[0..4] == b"\xd0\xcf\x11\xe0")
    {
        "OLE compound document"
    }
    else if data
        .iter()
        .take(4096)
        .all(|&b| b == 9 || b == 10 || b == 13 || (32..127).contains(&b))
    {
        "text"
    }
    else
    {
        "unrecognised"
    };
    let short = match label
    {
        "ar archive" => "ar",
        "OLE compound document" => "ole",
        "Android dex" => "dex",
        "unrecognised" => "raw",
        other => other,
    };
    Identity {
        label: label.into(),
        short: short.to_lowercase(),
        mime: None,
        precompressed: false,
    }
}

pub struct Container
{
    pub format: String,
    pub arch: String,
    pub kind: String,
    pub sections: Vec<Section>,
    pub entry: Option<u64>,
    pub symbols: Vec<Symbol>,
    pub functions: Vec<Function>,
    pub imports: Vec<Import>,
    pub exports: Vec<Export>,
}

fn archName(a: object::Architecture) -> &'static str
{
    use object::Architecture as A;
    match a
    {
        A::X86_64 => "x86-64",
        A::I386 => "x86",
        A::Aarch64 => "aarch64",
        A::Arm => "arm",
        A::Riscv64 => "riscv64",
        A::Riscv32 => "riscv32",
        A::PowerPc64 => "ppc64",
        A::PowerPc => "ppc",
        A::Mips64 => "mips64",
        A::Mips => "mips",
        A::Sparc64 => "sparc64",
        _ => "unknown",
    }
}

fn formatName(f: object::BinaryFormat) -> &'static str
{
    use object::BinaryFormat as F;
    match f
    {
        F::Elf => "ELF",
        F::Pe => "PE",
        F::Coff => "COFF",
        F::MachO => "Mach-O",
        F::Wasm => "Wasm",
        F::Xcoff => "XCOFF",
        _ => "object",
    }
}

fn kindName(k: object::ObjectKind) -> &'static str
{
    use object::ObjectKind as K;
    match k
    {
        K::Executable => "executable",
        K::Dynamic => "shared library",
        K::Relocatable => "object/BOF",
        K::Core => "core dump",
        _ => "unknown",
    }
}

fn symbolKindName(k: SymbolKind) -> &'static str
{
    match k
    {
        SymbolKind::Text => "text",
        SymbolKind::Data => "data",
        SymbolKind::Label => "label",
        SymbolKind::Tls => "tls",
        SymbolKind::Unknown => "unknown",
        SymbolKind::Section => "section",
        SymbolKind::File => "file",
        _ => "other",
    }
}

fn displayBytes(bytes: &[u8]) -> String
{
    String::from_utf8_lossy(bytes).into_owned()
}

fn displayName(name: NameOrOrdinal<&[u8]>) -> (String, Option<u16>)
{
    match name
    {
        NameOrOrdinal::Name(bytes) => (displayBytes(bytes), None),
        NameOrOrdinal::Ordinal(n) => (format!("#{}", n), Some(n)),
    }
}

/// Parse a container; None for anything without a recognised header, which callers treat as a raw blob.
pub fn parse(data: &[u8]) -> Option<Container>
{
    let file = object::File::parse(data).ok()?;

    let mut sections = Vec::new();
    let mut section_positions = HashMap::new();
    for s in file.sections()
    {
        let size = s.size();
        if size == 0
        {
            continue;
        }
        let (offset, nobits, bytes) = match s.file_range()
        {
            Some((off, len)) if len > 0 =>
            {
                let start = off as usize;
                let end = (off + len).min(data.len() as u64) as usize;
                (off, false, &data[start..end])
            }
            // NOBITS (.bss) or a section whose bytes aren't in the file.
            _ => (0, true, &[][..]),
        };

        // Raw bit values, to sidestep object's endian-newtype flag constants.
        const SHF_WRITE: u64 = 0x1;
        const SHF_EXECINSTR: u64 = 0x4;
        const SCN_MEM_EXECUTE: u32 = 0x2000_0000;
        const SCN_MEM_WRITE: u32 = 0x8000_0000;
        let (executable, writable) = match s.flags()
        {
            SectionFlags::Elf { sh_flags, .. } =>
            {
                let f: u64 = sh_flags.0;
                (f & SHF_EXECINSTR != 0, f & SHF_WRITE != 0)
            }
            SectionFlags::Coff {
                characteristics, ..
            } =>
            {
                let f: u32 = characteristics.0;
                (f & SCN_MEM_EXECUTE != 0, f & SCN_MEM_WRITE != 0)
            }
            SectionFlags::MachO { flags, .. } =>
            {
                let f: u32 = flags.0;
                (f & (0x0000_0400 | 0x8000_0000) != 0, false)
            }
            _ => (false, false),
        };

        section_positions.insert(s.index().0, sections.len());
        sections.push(Section {
            name: s.name().unwrap_or("<unnamed>").to_string(),
            offset,
            size,
            metrics: if nobits
            {
                Metrics::default()
            }
            else
            {
                Metrics::of(bytes)
            },
            executable,
            writable,
            symbols: 0,
            functions: 0,
            nobits,
        });
    }

    let mut symbols = Vec::new();
    let mut functions = Vec::new();
    let mut seen_symbols = HashSet::new();
    for symbol in file.symbols().chain(file.dynamic_symbols())
    {
        let kind = symbol.kind();
        if matches!(kind, SymbolKind::File | SymbolKind::Section)
        {
            continue;
        }
        let name = symbol.name().unwrap_or("").to_string();
        if name.is_empty()
        {
            continue;
        }
        if !seen_symbols.insert((name.clone(), symbol.address(), symbolKindName(kind)))
        {
            continue;
        }
        let section_index = symbol.section_index().map(|i| i.0);
        let section_pos = section_index.and_then(|i| section_positions.get(&i).copied());
        let section = section_pos.map(|i| sections[i].name.clone());
        if let Some(i) = section_pos
        {
            sections[i].symbols += 1;
        }

        let is_function = kind == SymbolKind::Text && symbol.is_definition();
        if is_function
        {
            let file_offset = symbol.section_index().and_then(|index| {
                let sec = file.section_by_index(index).ok()?;
                let (off, len) = sec.file_range()?;
                let delta = symbol.address().checked_sub(sec.address())?;
                (delta < len).then_some(off + delta)
            });
            if let Some(i) = section_pos
            {
                sections[i].functions += 1;
            }
            functions.push(Function {
                name: name.clone(),
                address: symbol.address(),
                size: symbol.size(),
                file_offset,
                section: section.clone(),
                global: symbol.is_global(),
            });
        }
        symbols.push(Symbol {
            name,
            address: symbol.address(),
            size: symbol.size(),
            kind: symbolKindName(kind).to_string(),
            section,
            global: symbol.is_global(),
        });
    }

    let imports = file
        .imports()
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|import| {
            let (name, ordinal) = displayName(import.name());
            Import {
                library: displayBytes(import.library()),
                name,
                ordinal,
                weak: import.is_weak(),
            }
        })
        .collect();

    let exports =
        file.exports()
            .ok()
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|export| {
                let (name, ordinal) = displayName(export.name());
                let target = export.target();
                let address =
                    match target
                    {
                        ExportTarget::Address { address }
                        | ExportTarget::TlvDescriptor { address } => Some(address),
                        ExportTarget::Resolver { resolver, .. } => Some(resolver),
                        _ => None,
                    };
                Export {
                    name,
                    ordinal,
                    address,
                    reexport: matches!(target, ExportTarget::Reexport { .. }),
                    weak: export.is_weak(),
                }
            })
            .collect();

    Some(Container {
        format: formatName(file.format()).to_string(),
        arch: archName(file.architecture()).to_string(),
        kind: kindName(file.kind()).to_string(),
        sections,
        entry: if file.entry() != 0
        {
            Some(file.entry())
        }
        else
        {
            None
        },
        symbols,
        functions,
        imports,
        exports,
    })
}
