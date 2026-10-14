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

#![allow(non_snake_case)]
///////////////////////////////////////////////////////////////////
// sprism (ShannonsPrism) — measure a file against many compression algorithms.
////////////////////////////////////////////////////////////

mod carve;
mod codecs;
mod corpus;
mod emap;
mod format;
mod libs;
mod pack;
mod registry;
mod score;

use clap::Parser;
use rayon::prelude::*;
use score::{Metrics, Row, SortKey, Verdict};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

// ---------------------------------------------------------------------------
// Jobs
// ---------------------------------------------------------------------------

struct Job
{
    name: String,
    native: Option<&'static str>,
    lib: Option<&'static str>,
}

fn buildJobs() -> Vec<Job>
{
    let mut jobs = vec![Job {
        name: "raw".into(),
        native: None,
        lib: None,
    }];

    let available: Vec<&str> = libs::available().iter().map(|c| c.name).collect();
    for codec in registry::ALL
    {
        if codec.backend == registry::Backend::Library && !available.contains(&codec.name)
        {
            continue;
        }
        jobs.push(Job {
            name: codec.name.into(),
            native: (codec.backend == registry::Backend::Native).then_some(codec.name),
            lib: (codec.backend == registry::Backend::Library).then_some(codec.name),
        });
    }

    jobs
}

fn runJob(
    job: &Job,
    data: &[u8],
    params: &codecs::Params,
    verify: bool,
) -> (Vec<u8>, f64, bool, String)
{
    let t0 = Instant::now();

    // The first stage borrows the input, so no job copies the whole file.
    let mut cur: Vec<u8>;
    if let Some(n) = job.native
    {
        match codecs::encodeOne(n, data, params)
        {
            Ok(v) => cur = v,
            Err(e) => return (Vec::new(), t0.elapsed().as_secs_f64(), false, e),
        }
        if let Some(l) = job.lib
        {
            match libs::encodeSafe(libs::find(l).unwrap(), &cur)
            {
                Ok(v) => cur = v,
                Err(e) => return (Vec::new(), t0.elapsed().as_secs_f64(), false, e),
            }
        }
    }
    else if let Some(l) = job.lib
    {
        match libs::encodeSafe(libs::find(l).unwrap(), data)
        {
            Ok(v) => cur = v,
            Err(e) => return (Vec::new(), t0.elapsed().as_secs_f64(), false, e),
        }
    }
    else
    {
        cur = data.to_vec();
    }
    let elapsed = t0.elapsed().as_secs_f64();

    if !verify
    {
        return (cur, elapsed, true, String::new());
    }

    let mut back = cur.clone();
    if let Some(l) = job.lib
    {
        match libs::decodeSafe(libs::find(l).unwrap(), &back)
        {
            Ok(v) => back = v,
            Err(e) => return (cur, elapsed, false, format!("decode failed: {}", e)),
        }
    }
    if let Some(n) = job.native
    {
        match codecs::decodeOne(n, &back, params)
        {
            Ok(v) => back = v,
            Err(e) => return (cur, elapsed, false, format!("decode failed: {}", e)),
        }
    }
    if back != data
    {
        return (
            cur,
            elapsed,
            false,
            format!(
                "roundtrip mismatch: {} bytes back, {} expected",
                back.len(),
                data.len()
            ),
        );
    }
    (cur, elapsed, true, String::new())
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

struct Style
{
    on: bool,
}

impl Style
{
    fn p(&self, t: &str, code: &str) -> String
    {
        if self.on
        {
            format!("\x1b[{}m{}\x1b[0m", code, t)
        }
        else
        {
            t.to_string()
        }
    }
    fn good(&self, t: &str) -> String
    {
        self.p(t, "32")
    }
    fn bad(&self, t: &str) -> String
    {
        self.p(t, "31")
    }
    fn warn(&self, t: &str) -> String
    {
        self.p(t, "33")
    }
    fn dim(&self, t: &str) -> String
    {
        self.p(t, "2")
    }
    fn bold(&self, t: &str) -> String
    {
        self.p(t, "1")
    }
    fn cyan(&self, t: &str) -> String
    {
        self.p(t, "36")
    }
    fn odd(&self, t: &str) -> String
    {
        self.p(t, "35")
    }
}

fn vlen(s: &str) -> usize
{
    let mut n = 0;
    let mut esc = false;
    for c in s.chars()
    {
        if esc
        {
            if c.is_ascii_alphabetic()
            {
                esc = false
            }
        }
        else if c == '\x1b'
        {
            esc = true;
        }
        else
        {
            n += 1;
        }
    }
    n
}

fn pad(s: &str, w: usize, right: bool) -> String
{
    let gap = " ".repeat(w.saturating_sub(vlen(s)));
    if right
    {
        format!("{}{}", gap, s)
    }
    else
    {
        format!("{}{}", s, gap)
    }
}

fn commas(n: usize) -> String
{
    let s = n.to_string();
    let mut o = String::new();
    for (i, c) in s.chars().enumerate()
    {
        if i > 0 && (s.len() - i) % 3 == 0
        {
            o.push(',')
        }
        o.push(c);
    }
    o
}

const COLS: &[(&str, usize, bool)] = &[
    ("codec", 24, false),
    ("verdict", 8, true),
    ("size", 12, true),
    ("ratio", 8, true),
    ("infoR", 8, true),
    ("H", 8, true),
    ("H/max", 7, true),
    ("print", 7, true),
    ("uniq", 5, true),
    ("time", 8, true),
];

fn render(rows: &[Row], st: &Style, args: &Args, width: usize)
{
    println!();
    println!(
        "{}",
        COLS.iter()
            .map(|(n, w, r)| pad(&st.bold(n), *w, *r))
            .collect::<Vec<_>>()
            .join(" ")
    );
    println!("{}", "-".repeat(width));

    let limit = if args.limit == 0
    {
        rows.len()
    }
    else
    {
        args.limit.min(rows.len())
    };
    let mut last: Option<Verdict> = None;

    for r in &rows[..limit]
    {
        if last.is_some() && last != Some(r.verdict)
        {
            println!("{}", st.dim(&"-".repeat(width)));
        }
        last = Some(r.verdict);

        let m = &r.metrics;
        let name = match r.verdict
        {
            Verdict::Hit => st.good(&r.name),
            Verdict::Broken => st.bad(&r.name),
            Verdict::Recode => st.dim(&r.name),
            _ => r.name.clone(),
        };
        let verdict = match r.verdict
        {
            Verdict::Hit => st.good(r.verdict.label()),
            Verdict::OverH => st.warn(r.verdict.label()),
            Verdict::Recode => st.odd(r.verdict.label()),
            Verdict::Broken => st.bad(r.verdict.label()),
            Verdict::Expand => st.dim(r.verdict.label()),
        };
        let hs = format!("{:7.3}", m.entropy);
        let h = if r.capped && m.unique < 256
        {
            st.odd(&hs)
        }
        else if m.entropy > args.targetHigh()
        {
            st.bad(&hs)
        }
        else if m.entropy < args.targetLow()
        {
            st.cyan(&hs)
        }
        else
        {
            st.good(&hs)
        };
        let is = format!("{:7.3}", r.info_ratio);
        let info = if r.info_ratio < 0.5
        {
            st.good(&is)
        }
        else if r.info_ratio >= 0.98
        {
            st.odd(&is)
        }
        else
        {
            is
        };

        println!(
            "{} {} {} {} {} {} {} {} {} {}",
            pad(&name, COLS[0].1, false),
            pad(&verdict, COLS[1].1, true),
            pad(&commas(m.size), COLS[2].1, true),
            pad(&format!("{:7.3}", r.ratio), COLS[3].1, true),
            pad(&info, COLS[4].1, true),
            pad(&h, COLS[5].1, true),
            pad(&format!("{:6.3}", m.entropyNorm()), COLS[6].1, true),
            pad(&format!("{:5.1}%", 100.0 * m.printable), COLS[7].1, true),
            pad(&m.unique.to_string(), COLS[8].1, true),
            pad(&format!("{:.3}s", r.elapsed), COLS[9].1, true),
        );
        if !r.note.is_empty()
        {
            println!("{}", st.dim(&format!("    {}", r.note)));
        }
    }
    println!("{}", "-".repeat(width));
    if limit < rows.len()
    {
        println!(
            "{}",
            st.dim(&format!(
                "({} more rows; --limit 0 for all)",
                rows.len() - limit
            ))
        );
    }
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Parser, Clone)]
#[command(
    name = "sprism",
    version,
    about = "ShannonsPrism: compare compression codecs and report file structure."
)]
struct Args
{
    /// File to analyse, or a directory to scan as a corpus
    #[arg(required_unless_present_any = ["list", "unpack"])]
    file: Option<PathBuf>,

    /// Structural report: entropy map, per-section entropy and best codec
    #[arg(long)]
    sections: bool,

    /// Limit --sections and --pack to one section by name (repeatable)
    #[arg(long, value_name = "NAME")]
    section: Vec<String>,

    /// List the section names in the file and exit
    #[arg(long)]
    list_sections: bool,

    /// Pack the file to FILE.epk; with --sections, one codec per section
    #[arg(long)]
    pack: bool,

    /// Emit a small C decoder and its companion packed .epk
    #[arg(long)]
    emit_c: bool,

    /// Output path for --pack or --emit-c
    #[arg(long, value_name = "PATH")]
    out: Option<PathBuf>,

    /// Unpack a .epk back to the original bytes (writes to --out, or stdout)
    #[arg(long, value_name = "FILE.epk")]
    unpack: Option<PathBuf>,

    /// Find and decode embedded compressed streams; --out DIR saves payloads
    #[arg(long)]
    carve: bool,

    /// Zoom in on one region: hex, entropy per window, and what packs it
    #[arg(long, value_name = "OFFSET")]
    at: Option<String>,

    /// Bytes to cover from --at
    #[arg(long, default_value_t = 512, value_name = "N")]
    len: usize,

    /// Codec choice per piece for --sections, --pack and --at
    #[arg(long, default_value = "size", value_parser = ["size", "low-entropy", "high-entropy"])]
    pick: String,

    /// Entropy band in bits per byte for the hit/over-H verdict
    #[arg(long, default_value = "4:6", value_name = "LOW:HIGH")]
    band: String,

    /// Sort order for the codec table
    #[arg(long, default_value = "score",
          value_parser = ["score", "info", "ratio", "size", "entropy", "time", "name"])]
    sort: String,

    /// Rows to print; 0 for all
    #[arg(long, default_value_t = 30)]
    limit: usize,

    /// Emit machine-readable JSON instead of a table
    #[arg(long)]
    json: bool,

    /// Worker threads
    #[arg(long)]
    threads: Option<usize>,

    /// Refuse inputs larger than this many MB (0 disables)
    #[arg(long, default_value_t = 512, value_name = "MB")]
    max_size: usize,

    /// Include codecs with impractical runtimes on large whole-file sweeps
    #[arg(long)]
    exhaustive: bool,

    /// List available codecs and exit
    #[arg(long)]
    list: bool,
}

impl Args
{
    /// Parse --band "low:high"; falls back to 4:6 on malformed input.
    fn bandEdges(&self) -> (f64, f64)
    {
        let mut parts = self.band.splitn(2, ':');
        let lo = parts
            .next()
            .and_then(|x| x.trim().parse().ok())
            .unwrap_or(4.0);
        let hi = parts
            .next()
            .and_then(|x| x.trim().parse().ok())
            .unwrap_or(6.0);
        if hi >= lo
        {
            (lo, hi)
        }
        else
        {
            (hi, lo)
        }
    }
    fn pick(&self) -> Pick
    {
        match self.pick.as_str()
        {
            "low-entropy" => Pick::LowEntropy,
            "high-entropy" => Pick::HighEntropy,
            _ => Pick::Size,
        }
    }
    fn targetLow(&self) -> f64
    {
        self.bandEdges().0
    }
    fn targetHigh(&self) -> f64
    {
        self.bandEdges().1
    }

    /// Map window aiming for a few thousand blocks at any file size.
    fn mapWindow(&self, len: usize) -> usize
    {
        (len / 2048).clamp(64, 65536)
    }
}

fn jsonStr(s: &str) -> String
{
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars()
    {
        match c
        {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// Strip control characters so a crafted filename cannot inject terminal escapes.
fn safePath(p: &std::path::Path) -> String
{
    p.display()
        .to_string()
        .chars()
        .map(|c| {
            if (c as u32) < 0x20 || c == '\x7f'
            {
                '\u{fffd}'
            }
            else
            {
                c
            }
        })
        .collect()
}

fn bandColor(st: &Style, b: emap::Band, text: &str) -> String
{
    match b
    {
        emap::Band::Padding => st.dim(text),
        emap::Band::Structured => st.cyan(text),
        emap::Band::Code => st.good(text),
        emap::Band::Compressed => st.warn(text),
        emap::Band::Random => st.bad(text),
    }
}

// ---------------------------------------------------------------------------
// Map overview: strip, ruler, hot regions. Shared by --sections and --carve.
// ---------------------------------------------------------------------------

/// Adjacent Compressed/Random map regions merged into one span (window noise fragments one payload).
struct Hot
{
    start: usize,
    end: usize,
    mean_entropy: f64,
    band: emap::Band,
    uniformity: Option<emap::Uniformity>,
}

/// Hot spans at least two windows long, so a single noisy block does not make the list.
fn hotRegions(map: &emap::EntropyMap, data: &[u8]) -> Vec<Hot>
{
    let mut spans: Vec<(usize, usize, f64)> = Vec::new(); // start, end, sum(H * len)
    for r in &map.regions
    {
        if !matches!(r.band, emap::Band::Random | emap::Band::Compressed)
        {
            continue;
        }
        let w = r.mean_entropy * r.len() as f64;
        match spans.last_mut()
        {
            Some(last) if last.1 == r.start =>
            {
                last.1 = r.end;
                last.2 += w;
            }
            _ => spans.push((r.start, r.end, w)),
        }
    }
    spans
        .into_iter()
        .filter(|(a, b, _)| b - a >= (map.window * 2).max(emap::MIN_JUDGEABLE))
        .filter_map(|(start, end, _)| {
            let end = end.min(data.len());
            // Small windows overstate mid-range entropy; re-judge the whole span.
            let mean = score::entropyOf(&data[start..end]);
            let band = emap::Band::ofWindowed(mean, end - start);
            if !matches!(band, emap::Band::Random | emap::Band::Compressed)
            {
                return None;
            }
            let uniformity = if end - start >= emap::MIN_CHI2_BYTES
            {
                Some(emap::uniformity(&data[start..end]))
            }
            else
            {
                None
            };
            Some(Hot {
                start,
                end,
                mean_entropy: mean,
                band,
                uniformity,
            })
        })
        .collect()
}

/// Whole-file strip with an address ruler, then the hot spans; `streams` are carve results, if any.
fn printMapOverview(data: &[u8], smap: &emap::EntropyMap, st: &Style, streams: &[carve::Found])
{
    let strip_w = 72.min(smap.blocks.len().max(1));
    let cells = emap::strip(smap, strip_w);
    let strip: String = cells
        .iter()
        .map(|b| bandColor(st, *b, &b.glyph().to_string()))
        .collect();
    println!();
    // Ruler ticks at 0, 1/4, 1/2, 3/4, end.
    let sw = cells.len();
    println!("  {}", strip);
    {
        let mut ruler = vec![b' '; sw];
        let mut labels: Vec<(usize, String)> = Vec::new();
        for k in 0..=4
        {
            let col = (k * (sw.saturating_sub(1))) / 4;
            let off = (k * data.len()) / 4;
            if col < sw
            {
                ruler[col] = b'^';
            }
            labels.push((col, format!("0x{:x}", off)));
        }
        println!("  {}", st.dim(&String::from_utf8_lossy(&ruler)));
        let mut line = vec![b' '; sw + 12];
        for (col, lab) in &labels
        {
            let start = (*col).min(line.len().saturating_sub(lab.len()));
            for (j, ch) in lab.bytes().enumerate()
            {
                if start + j < line.len() && line[start + j] == b' '
                {
                    line[start + j] = ch;
                }
            }
        }
        println!("  {}", st.dim(&String::from_utf8_lossy(&line).trim_end()));
    }

    let hot = hotRegions(smap, data);
    let container = format::parse(data);
    println!(
        "{}",
        st.dim(&format!(
            "  window {} step {}   {} blocks",
            smap.window,
            smap.step,
            smap.blocks.len()
        ))
    );
    if !hot.is_empty()
    {
        println!(
            "{}",
            st.bold("regions worth a closer look (entropy/--at target):")
        );
        for r in hot.iter().take(8)
        {
            let sec = container.as_ref().and_then(|c| {
                c.sections
                    .iter()
                    .find(|s| {
                        !s.nobits
                            && r.start >= s.offset as usize
                            && r.start < (s.offset + s.size) as usize
                    })
                    .map(|s| s.name.as_str())
            });
            // A stream header inside the span overrides the chi-square verdict:
            // range-coder output is as flat as ciphertext.
            let inside: Vec<&carve::Found> = streams
                .iter()
                .filter(|f| {
                    f.offset + f.stream_len > r.start.saturating_sub(smap.window)
                        && f.offset < r.end
                })
                .collect();
            let verdict = match r.uniformity
            {
                Some(u) if r.band == emap::Band::Random && !inside.is_empty() => format!(
                    "{} ({}, but has a stream header)",
                    bandColor(st, r.band, "packed"),
                    u.summary()
                ),
                Some(u) if r.band == emap::Band::Random => format!(
                    "{} ({})",
                    bandColor(st, r.band, r.band.describe(r.uniformity)),
                    u.summary()
                ),
                _ => bandColor(st, r.band, r.band.describe(r.uniformity)),
            };
            println!(
                "  {} {}  mean H {:.2}  {}{}",
                st.bold(&format!("--at 0x{:x}", r.start)),
                pad(
                    &format!(
                        "(0x{:x}-0x{:x}, {} bytes)",
                        r.start,
                        r.end,
                        commas(r.end - r.start)
                    ),
                    34,
                    false
                ),
                r.mean_entropy,
                verdict,
                sec.map(|n| format!(" in {}", n)).unwrap_or_default()
            );
            for f in inside.iter().take(4)
            {
                let what = f.container.clone().unwrap_or_else(|| f.identity.clone());
                let rel = if f.offset + smap.window < r.start
                {
                    "inside the"
                }
                else
                {
                    "has"
                };
                println!(
                    "    {}",
                    st.dim(&format!(
                        "{} {} stream at 0x{:x}: {} -> {} ({})",
                        rel,
                        f.codec,
                        f.offset,
                        carve::human(f.stream_len),
                        carve::human(f.decoded_len),
                        what
                    ))
                );
            }
            if inside.len() > 4
            {
                println!(
                    "    {}",
                    st.dim(&format!("({} more streams)", inside.len() - 4))
                );
            }
        }
        if hot.len() > 8
        {
            println!("  {}", st.dim(&format!("({} more)", hot.len() - 8)));
        }
    }
}

// ---------------------------------------------------------------------------
// --sections
// ---------------------------------------------------------------------------

/// Parse the container and keep only the sections named by --section (all when none given).
fn parseFiltered(data: &[u8], args: &Args) -> Option<format::Container>
{
    let mut c = format::parse(data)?;
    if !args.section.is_empty()
    {
        c.sections
            .retain(|s| args.section.iter().any(|want| want == &s.name));
    }
    Some(c)
}

fn runListSections(path: &std::path::Path, data: &[u8], st: &Style) -> std::process::ExitCode
{
    match format::parse(data)
    {
        Some(c) =>
        {
            println!("{}", st.bold(&safePath(path)));
            for sec in &c.sections
            {
                println!(
                    "  {} {} {}",
                    pad(&sec.name, 22, false),
                    pad(&format!("0x{:x}", sec.offset), 10, true),
                    pad(&commas(sec.size as usize), 10, true)
                );
            }
            std::process::ExitCode::SUCCESS
        }
        None =>
        {
            eprintln!(
                "sprism: {}: no section table (not ELF, PE, COFF, Mach-O, Wasm, or XCOFF)",
                safePath(path)
            );
            std::process::ExitCode::from(1)
        }
    }
}

fn runSections(
    path: &std::path::Path,
    data: &[u8],
    args: &Args,
    st: &Style,
) -> std::process::ExitCode
{
    let container = match parseFiltered(data, args)
    {
        Some(c) if !c.sections.is_empty() => c,
        Some(_) =>
        {
            eprintln!("sprism: no section matched --section {:?}", args.section);
            return std::process::ExitCode::from(1);
        }
        None =>
        {
            if args.json
            {
                println!(
                    "{{\"path\":{},\"format\":\"raw\",\"sections\":[]}}",
                    jsonStr(&safePath(path))
                );
            }
            else
            {
                eprintln!(
                    "sprism: {}: no ELF, PE, COFF, Mach-O, Wasm, or XCOFF header — try --carve for a flat blob",
                    safePath(path)
                );
            }
            return std::process::ExitCode::from(1);
        }
    };
    let whole = Metrics::of(data);

    if args.json
    {
        let params = codecs::Params::default();
        let jobs = sweepJobs();
        let jfits: Vec<Option<BestFit>> = container
            .sections
            .iter()
            .map(|sec| {
                if sec.nobits
                {
                    return None;
                }
                let start = sec.offset as usize;
                let end = (start + sec.size as usize).min(data.len());
                if start >= end
                {
                    return None;
                }
                sweepBest(
                    &data[start..end],
                    &jobs,
                    &params,
                    true,
                    args.targetLow(),
                    args.targetHigh(),
                    args.pick(),
                )
            })
            .collect();

        let mut secs = Vec::new();
        for (i, s) in container.sections.iter().enumerate()
        {
            let fit = jfits
                .get(i)
                .and_then(|f| f.as_ref())
                .map(|f| {
                    format!(
                ",\"best_codec\":{},\"packed_size\":{},\"packed_ratio\":{:.4},\"decoder\":{}",
                jsonStr(&f.codec), f.size, f.ratio, jsonStr(&pack::decoderHint(&f.codec))
            )
                })
                .unwrap_or_default();
            let band = emap::Band::classifySection(s.metrics.entropy, s.size as usize);
            let chi = if band == Some(emap::Band::Random) && !s.nobits
            {
                let start = s.offset as usize;
                let end = (start + s.size as usize).min(data.len());
                if end > start && end - start >= emap::MIN_CHI2_BYTES
                {
                    let u = emap::uniformity(&data[start..end]);
                    format!(
                        ",\"chi2\":{:.2},\"chi2_p\":{:.4e},\"uniform\":{}",
                        u.chi2,
                        u.p,
                        u.isUniform()
                    )
                }
                else
                {
                    String::new()
                }
            }
            else
            {
                String::new()
            };
            secs.push(format!(
                "{{\"name\":{},\"offset\":{},\"size\":{},\"entropy\":{:.4},\"band\":{},\"exec\":{},\"write\":{},\"nobits\":{},\"symbol_count\":{},\"function_count\":{}{}{}}}",
                jsonStr(&s.name), s.offset, s.size, s.metrics.entropy,
                band.map(|b| jsonStr(b.label())).unwrap_or_else(|| "null".into()),
                s.executable, s.writable, s.nobits, s.symbols, s.functions, chi, fit
            ));
        }
        let ident = format::identify(data);
        let imports = container
            .imports
            .iter()
            .map(|i| {
                format!(
                    "{{\"library\":{},\"name\":{},\"ordinal\":{},\"weak\":{}}}",
                    jsonStr(&i.library),
                    jsonStr(&i.name),
                    i.ordinal
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "null".into()),
                    i.weak
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let exports = container
            .exports
            .iter()
            .map(|e| {
                format!(
                    "{{\"name\":{},\"ordinal\":{},\"address\":{},\"reexport\":{},\"weak\":{}}}",
                    jsonStr(&e.name),
                    e.ordinal
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "null".into()),
                    e.address
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "null".into()),
                    e.reexport,
                    e.weak
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let symbols = container
            .symbols
            .iter()
            .map(|s| {
                format!(
                    "{{\"name\":{},\"address\":{},\"size\":{},\"kind\":{},\"section\":{},\"global\":{}}}",
                    jsonStr(&s.name), s.address, s.size, jsonStr(&s.kind),
                    s.section.as_ref().map(|n| jsonStr(n)).unwrap_or_else(|| "null".into()),
                    s.global
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let functions = container
            .functions
            .iter()
            .map(|f| {
                format!(
                    "{{\"name\":{},\"address\":{},\"size\":{},\"file_offset\":{},\"section\":{},\"global\":{}}}",
                    jsonStr(&f.name), f.address, f.size,
                    f.file_offset.map(|n| n.to_string()).unwrap_or_else(|| "null".into()),
                    f.section.as_ref().map(|n| jsonStr(n)).unwrap_or_else(|| "null".into()),
                    f.global
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        println!(
            "{{\"path\":{},\"format\":{},\"arch\":{},\"kind\":{},\"identified\":{},\"mime\":{},\"precompressed\":{},\"size\":{},\"entropy\":{:.4},\"entry\":{},\"symbol_count\":{},\"function_count\":{},\"import_count\":{},\"export_count\":{},\"imports\":[{}],\"exports\":[{}],\"symbols\":[{}],\"functions\":[{}],\"sections\":[{}]}}",
            jsonStr(&safePath(path)), jsonStr(&container.format), jsonStr(&container.arch),
            jsonStr(&container.kind), jsonStr(&ident.label),
            ident.mime.as_ref().map(|m| jsonStr(m)).unwrap_or_else(|| "null".into()),
            ident.precompressed,
            data.len(), whole.entropy,
            container.entry.map(|n| n.to_string()).unwrap_or_else(|| "null".into()),
            container.symbols.len(), container.functions.len(), container.imports.len(),
            container.exports.len(), imports, exports, symbols, functions, secs.join(",")
        );
        return std::process::ExitCode::SUCCESS;
    }

    let ident = format::identify(data);
    println!("{}", st.bold(&safePath(path)));
    println!(
        "{} {} {}   {} bytes   entry {}   {} symbols   {} functions   {} imports   {} exports",
        container.format,
        container.arch,
        container.kind,
        commas(data.len()),
        container
            .entry
            .map(|e| format!("0x{:x}", e))
            .unwrap_or_else(|| "-".into()),
        container.symbols.len(),
        container.functions.len(),
        container.imports.len(),
        container.exports.len()
    );
    println!("{}", st.dim(&format!("identified as: {}", ident.label)));

    let map_window = args.mapWindow(data.len());
    let smap = emap::compute(data, map_window, map_window.max(1));
    printMapOverview(data, &smap, st, &[]);
    println!("{}", st.dim(&format!("whole-file H {:.3}", whole.entropy)));
    if args.pick() != Pick::Size
    {
        println!(
            "{}",
            st.dim(&format!(
                "codec per section: {} output (--pick {})",
                args.pick().label(),
                args.pick
            ))
        );
    }

    let params = codecs::Params::default();
    let fits: Vec<Option<BestFit>> = {
        let jobs = sweepJobs();
        let total = container
            .sections
            .iter()
            .filter(|s| !s.nobits && s.size > 0)
            .count();
        let show_progress = st.on && total > 4;
        let mut done = 0usize;
        let out: Vec<Option<BestFit>> = container
            .sections
            .iter()
            .map(|sec| {
                if sec.nobits
                {
                    return None;
                }
                let start = sec.offset as usize;
                let end = (start + sec.size as usize).min(data.len());
                if start >= end
                {
                    return None;
                }
                let r = sweepBest(
                    &data[start..end],
                    &jobs,
                    &params,
                    true,
                    args.targetLow(),
                    args.targetHigh(),
                    args.pick(),
                );
                if show_progress
                {
                    done += 1;
                    let w = 30;
                    let filled = w * done / total;
                    eprint!(
                        "\r  sweeping {} codecs  [{}{}] {}/{} {}",
                        jobs.len(),
                        "=".repeat(filled),
                        " ".repeat(w - filled),
                        done,
                        total,
                        sec.name
                    );
                }
                r
            })
            .collect();
        if show_progress
        {
            eprint!("\r\x1b[2K");
        }
        out
    };

    println!();
    println!(
        "{} {} {} {} {} {} {} {} {} {} {} {}",
        pad(&st.bold("section"), 18, false),
        pad(&st.bold("offset"), 10, true),
        pad(&st.bold("size"), 9, true),
        pad(&st.bold("H"), 8, true),
        pad(&st.bold("fl"), 3, false),
        pad(&st.bold("sym"), 6, true),
        pad(&st.bold("func"), 6, true),
        pad(&st.bold("band"), 11, false),
        pad(&st.bold("best codec"), 16, false),
        pad(&st.bold("packed"), 9, true),
        pad(&st.bold("ratio"), 7, true),
        st.bold("packed H")
    );
    let w = 18 + 10 + 9 + 8 + 3 + 6 + 6 + 11 + 16 + 9 + 7 + 8 + 11;
    println!("{}", "-".repeat(w));

    for (idx, s) in container.sections.iter().enumerate()
    {
        if s.nobits
        {
            println!(
                "{} {} {} {} {} {} {}",
                pad(&s.name, 18, false),
                pad(&format!("0x{:x}", s.offset), 10, true),
                pad(&commas(s.size as usize), 9, true),
                pad("-", 8, true),
                pad(&s.symbols.to_string(), 6, true),
                pad(&s.functions.to_string(), 6, true),
                st.dim("  no file bytes (.bss)")
            );
            continue;
        }
        // Too small a sample to classify -> None -> dash, not a wrong label.
        let band_opt = emap::Band::classifySection(s.metrics.entropy, s.size as usize);
        let hs = format!("{:6.3}", s.metrics.entropy);
        let h = match band_opt
        {
            Some(b) => bandColor(st, b, &hs),
            None => st.dim(&hs),
        };
        let uni = if band_opt == Some(emap::Band::Random)
        {
            let start = s.offset as usize;
            let end = (start + s.size as usize).min(data.len());
            if end > start && end - start >= emap::MIN_CHI2_BYTES
            {
                Some(emap::uniformity(&data[start..end]))
            }
            else
            {
                None
            }
        }
        else
        {
            None
        };
        let band_cell = match band_opt
        {
            Some(b) => bandColor(st, b, b.describe(uni)),
            None => st.dim("  -"),
        };
        let mut flags = String::new();
        flags.push(if s.executable { 'x' } else { '-' });
        flags.push(if s.writable { 'w' } else { '-' });
        let note = if s.executable && s.metrics.entropy >= 7.0
        {
            st.bad("  <- exec + high-entropy: packed/encrypted code?")
        }
        else
        {
            String::new()
        };
        let (best, packed, ratio, packed_h) = match fits.get(idx).and_then(|f| f.as_ref())
        {
            Some(f) =>
            {
                let r = format!("{:.3}", f.ratio);
                let r = if f.ratio < 0.5
                {
                    st.good(&r)
                }
                else if f.ratio >= 1.0
                {
                    st.dim(&r)
                }
                else
                {
                    r
                };
                let pb = emap::Band::ofWindowed(f.entropy, f.size.max(1));
                let ph = bandColor(st, pb, &format!("{:6.3}", f.entropy));
                (f.codec.clone(), commas(f.size), r, ph)
            }
            None => (st.dim("(none)"), "-".into(), "-".into(), "-".into()),
        };
        println!(
            "{} {} {} {} {} {} {} {} {} {} {} {}{}",
            pad(&s.name, 18, false),
            pad(&format!("0x{:x}", s.offset), 10, true),
            pad(&commas(s.size as usize), 9, true),
            pad(&h, 8, true),
            pad(&flags, 3, false),
            pad(&s.symbols.to_string(), 6, true),
            pad(&s.functions.to_string(), 6, true),
            pad(&band_cell, 11, false),
            pad(&best, 16, false),
            pad(&packed, 9, true),
            pad(&ratio, 7, true),
            pad(&packed_h, 8, true),
            note
        );
    }
    println!("{}", "-".repeat(w));

    let metadata_limit = |n: usize| {
        if args.limit == 0
        {
            n
        }
        else
        {
            n.min(args.limit)
        }
    };
    if !container.imports.is_empty()
    {
        println!();
        println!(
            "{}",
            st.bold(&format!("imports ({})", container.imports.len()))
        );
        for import in container
            .imports
            .iter()
            .take(metadata_limit(container.imports.len()))
        {
            let source = if import.library.is_empty()
            {
                "(unspecified)"
            }
            else
            {
                &import.library
            };
            println!("  {}!{}", source, import.name);
        }
        if metadata_limit(container.imports.len()) < container.imports.len()
        {
            println!(
                "  {}",
                st.dim(&format!(
                    "({} more; use --limit 0 for all)",
                    container.imports.len() - metadata_limit(container.imports.len())
                ))
            );
        }
    }
    if !container.exports.is_empty()
    {
        println!();
        println!(
            "{}",
            st.bold(&format!("exports ({})", container.exports.len()))
        );
        for export in container
            .exports
            .iter()
            .take(metadata_limit(container.exports.len()))
        {
            let at = export
                .address
                .map(|a| format!("0x{:x}", a))
                .unwrap_or_else(|| {
                    if export.reexport
                    {
                        "re-export".into()
                    }
                    else
                    {
                        "-".into()
                    }
                });
            println!("  {} {}", pad(&at, 18, false), export.name);
        }
        if metadata_limit(container.exports.len()) < container.exports.len()
        {
            println!(
                "  {}",
                st.dim(&format!(
                    "({} more; use --limit 0 for all)",
                    container.exports.len() - metadata_limit(container.exports.len())
                ))
            );
        }
    }

    {
        // Both sides must cover identical bytes: unsweepable sections count at
        // raw size, and the blob is the concatenation of the same sections.
        let mut concat: Vec<u8> = Vec::new();
        let mut per_section = 0usize;
        let mut covered = 0usize;
        let mut skipped = 0usize;
        for (i, sec) in container.sections.iter().enumerate()
        {
            if sec.nobits
            {
                continue;
            }
            let start = sec.offset as usize;
            let end = (start + sec.size as usize).min(data.len());
            if start >= end
            {
                continue;
            }
            concat.extend_from_slice(&data[start..end]);
            covered += end - start;
            match fits.get(i).and_then(|f| f.as_ref())
            {
                Some(f) => per_section += f.size,
                None =>
                {
                    per_section += end - start;
                    skipped += 1;
                }
            }
        }

        let jobs = sweepJobs();
        let blob_best = sweepBest(
            &concat,
            &jobs,
            &params,
            true,
            args.targetLow(),
            args.targetHigh(),
            args.pick(),
        );

        println!();
        println!("{}", st.bold("per-section vs one blob"));
        println!(
            "  {} bytes of section data, compared two ways:",
            commas(covered)
        );
        println!(
            "    per-section: {} bytes ({:.3}) — each section with its own best codec",
            commas(per_section),
            if covered > 0
            {
                per_section as f64 / covered as f64
            }
            else
            {
                0.0
            }
        );
        if let Some(b) = &blob_best
        {
            println!(
                "    single blob: {} bytes ({:.3}) — all sections concatenated, {} alone",
                commas(b.size),
                b.ratio,
                b.codec
            );
            let delta = b.size as f64 - per_section as f64;
            if delta > 0.0
            {
                println!("  {}", st.good(&format!(
                    "per-section wins by {} bytes: the sections differ enough that one codec cannot suit them all",
                    commas(delta as usize))));
            }
            else
            {
                println!("  {}", st.dim(&format!(
                    "one blob wins by {} bytes: cross-section redundancy and amortised headers beat per-section tuning",
                    commas((-delta) as usize))));
            }
        }
        if skipped > 0
        {
            println!(
                "  {}",
                st.dim(&format!(
                    "{} section(s) under {} bytes counted at raw size",
                    skipped, MIN_SWEEPABLE
                ))
            );
        }

        let swept: Vec<(&format::Section, &BestFit)> = container
            .sections
            .iter()
            .enumerate()
            .filter_map(|(i, sec)| fits.get(i).and_then(|f| f.as_ref()).map(|f| (sec, f)))
            .collect();
        let banded: Vec<_> = swept.iter().filter(|(_, f)| f.in_band.is_some()).collect();

        println!();
        println!(
            "{}",
            st.bold(&format!(
                "sections with a codec that compresses and stays in [{:.1}, {:.1}]",
                args.targetLow(),
                args.targetHigh()
            ))
        );
        if banded.is_empty()
        {
            println!(
                "  {}",
                st.dim(
                    "none — every codec that shrank a section pushed its entropy above the ceiling"
                )
            );
        }
        else
        {
            for (sec, f) in banded.iter().take(12)
            {
                let (codec, size, ratio) = f.in_band.as_ref().unwrap();
                println!(
                    "  {} {} -> {} ({:.3}) via {}",
                    pad(&sec.name, 20, false),
                    commas(sec.size as usize),
                    commas(*size),
                    ratio,
                    codec
                );
            }
            if banded.len() > 12
            {
                println!("  {}", st.dim(&format!("({} more)", banded.len() - 12)));
            }
            println!(
                "  {}",
                st.dim(&format!(
                    "{} of {} swept sections; the rest compress only by pushing entropy past {:.1}",
                    banded.len(),
                    swept.len(),
                    args.targetHigh()
                ))
            );
        }
    }

    println!();
    if args.carve
    {
        println!();
        return runCarve(path, data, args, st, false);
    }
    std::process::ExitCode::SUCCESS
}

// ---------------------------------------------------------------------------
// Corpus (directory input)
// ---------------------------------------------------------------------------

fn runCorpus(dir: &std::path::Path, args: &Args, st: &Style) -> std::process::ExitCode
{
    if !dir.is_dir()
    {
        eprintln!("sprism: {}: not a directory", safePath(dir));
        return std::process::ExitCode::from(2);
    }
    // 4096: a narrower window's sampling variance straddles band boundaries.
    let entries = corpus::analyse(dir, 4096, 100_000, args.threads);
    if entries.is_empty()
    {
        eprintln!("sprism: {}: no files", safePath(dir));
        return std::process::ExitCode::from(1);
    }

    if args.json
    {
        let rows: Vec<String> = entries.iter().map(|e| format!(
            "{{\"path\":{},\"size\":{},\"entropy\":{:.4},\"format\":{},\"dominant\":{},\"random_frac\":{:.4}{}}}",
            jsonStr(&safePath(&e.path)), e.size, e.entropy, jsonStr(&e.format),
            jsonStr(e.dominant.label()), e.random_frac,
            e.error.as_ref().map(|x| format!(",\"error\":{}", jsonStr(x))).unwrap_or_default()
        )).collect();
        println!("[{}]", rows.join(","));
        return std::process::ExitCode::SUCCESS;
    }

    let mut sorted: Vec<&corpus::CorpusEntry> = entries.iter().collect();
    // Most-random first: the outliers are what a corpus scan is for.
    sorted.sort_by(|a, b| {
        b.random_frac
            .partial_cmp(&a.random_frac)
            .unwrap()
            .then(b.entropy.partial_cmp(&a.entropy).unwrap())
    });

    println!(
        "{}",
        st.bold(&format!("{} — {} files", safePath(dir), entries.len()))
    );
    println!();
    println!(
        "{} {} {} {} {} {}",
        pad(&st.bold("file"), 40, false),
        pad(&st.bold("size"), 11, true),
        pad(&st.bold("H"), 7, true),
        pad(&st.bold("fmt"), 6, false),
        pad(&st.bold("rnd%"), 6, true),
        st.bold("dominant")
    );
    let w = 40 + 11 + 7 + 6 + 6 + 14;
    println!("{}", "-".repeat(w));

    let limit = if args.limit == 0
    {
        sorted.len()
    }
    else
    {
        args.limit.min(sorted.len())
    };
    for e in &sorted[..limit]
    {
        if let Some(err) = &e.error
        {
            println!(
                "{} {}",
                pad(&trunc(&safePath(&e.path), 40), 40, false),
                st.dim(err)
            );
            continue;
        }
        let band = e.dominant;
        // Path relative to the scanned root; bare names collide in deep trees.
        let rel = e.path.strip_prefix(dir).unwrap_or(&e.path);
        let name = trunc(&safePath(rel), 40);
        let fmt = if e.precompressed
        {
            st.odd(&e.format)
        }
        else
        {
            e.format.clone()
        };
        println!(
            "{} {} {} {} {} {}",
            pad(&name, 40, false),
            pad(&commas(e.size), 11, true),
            pad(&format!("{:6.3}", e.entropy), 7, true),
            pad(&fmt, 6, false),
            pad(&format!("{:.0}%", 100.0 * e.random_frac), 6, true),
            bandColor(st, band, band.label())
        );
    }
    println!("{}", "-".repeat(w));
    if limit < sorted.len()
    {
        println!(
            "{}",
            st.dim(&format!(
                "({} more; --limit 0 for all)",
                sorted.len() - limit
            ))
        );
    }

    {
        let n = entries.len() as f64;
        let mean = entries.iter().map(|e| e.entropy).sum::<f64>() / n;
        let packed = entries.iter().filter(|e| e.random_frac > 0.9).count();
        let known = entries.iter().filter(|e| e.precompressed).count();
        println!();
        println!(
            "mean whole-file H {:.3}   {} file(s) look fully random",
            mean, packed
        );
        if known > 0
        {
            println!(
                "{}",
                st.dim(&format!(
                    "{} of those are recognised compressed containers ({} in the fmt column); \
                 the rest are unexplained and worth a closer look",
                    known,
                    st.odd("highlighted")
                ))
            );
        }
    }
    std::process::ExitCode::SUCCESS
}

// ---------------------------------------------------------------------------
// Packing
// ---------------------------------------------------------------------------

/// Criterion for choosing one codec per piece; only codecs that shrink the piece qualify.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Pick
{
    Size,
    LowEntropy,
    HighEntropy,
}

impl Pick
{
    /// Does candidate (size, entropy) beat the incumbent?
    fn better(self, cand: (usize, f64), inc: (usize, f64)) -> bool
    {
        match self
        {
            Pick::Size => cand.0 < inc.0,
            Pick::LowEntropy => cand.1 < inc.1 || (cand.1 == inc.1 && cand.0 < inc.0),
            Pick::HighEntropy => cand.1 > inc.1 || (cand.1 == inc.1 && cand.0 < inc.0),
        }
    }
    fn label(self) -> &'static str
    {
        match self
        {
            Pick::Size => "smallest",
            Pick::LowEntropy => "lowest entropy",
            Pick::HighEntropy => "highest entropy",
        }
    }
}

/// Best codec for a chunk by the chosen criterion, optionally only those with a C decoder.
fn bestFor(
    data: &[u8],
    jobs: &[Job],
    params: &codecs::Params,
    portable_only: bool,
    pick: Pick,
) -> Option<(String, Vec<u8>)>
{
    let pool: Vec<&Job> = jobs
        .iter()
        .filter(|j| !portable_only || pack::isPortable(&j.name))
        .collect();
    let mut best: Option<(String, Vec<u8>, f64)> = None;
    for job in pool
    {
        let (out, _, ok, _) = runJob(job, data, params, true);
        if !ok || out.is_empty() || out.len() >= data.len()
        {
            continue;
        }
        let h = score::entropyOf(&out);
        let wins = match &best
        {
            Some((_, b, bh)) => pick.better((out.len(), h), (b.len(), *bh)),
            None => true,
        };
        if wins
        {
            best = Some((job.name.clone(), out, h));
        }
    }
    best.map(|(name, out, _)| (name, out))
}

/// Map pieces shorter than this are joined to their successor: a chunk header per region would eat the gain.
const MIN_PIECE: usize = 16 << 10;

/// Sectionless blobs: merge map regions on the same side of the hot line into pack pieces.
fn piecesFromMap(map: &emap::EntropyMap, len: usize) -> Vec<(String, usize, usize)>
{
    struct Piece
    {
        start: usize,
        end: usize,
        hot: bool,
        bytes: [usize; 5],
        label: [&'static str; 5],
    }
    let idx = |b: emap::Band| match b
    {
        emap::Band::Padding => 0,
        emap::Band::Structured => 1,
        emap::Band::Code => 2,
        emap::Band::Compressed => 3,
        emap::Band::Random => 4,
    };
    let mut pieces: Vec<Piece> = Vec::new();
    for r in &map.regions
    {
        let end = r.end.min(len);
        if end <= r.start
        {
            continue;
        }
        let hot = matches!(r.band, emap::Band::Compressed | emap::Band::Random);
        let join = match pieces.last()
        {
            Some(p) => p.hot == hot || p.end - p.start < MIN_PIECE,
            None => false,
        };
        if join
        {
            let p = pieces.last_mut().unwrap();
            p.end = end;
            p.bytes[idx(r.band)] += end - r.start;
            p.label[idx(r.band)] = r.describe();
            // A piece grown past the minimum takes the side of what it holds.
            let hot_bytes = p.bytes[3] + p.bytes[4];
            p.hot = hot_bytes * 2 > p.end - p.start;
        }
        else
        {
            let mut bytes = [0usize; 5];
            let mut label = [""; 5];
            bytes[idx(r.band)] = end - r.start;
            label[idx(r.band)] = r.describe();
            pieces.push(Piece {
                start: r.start,
                end,
                hot,
                bytes,
                label,
            });
        }
    }
    // A short trailing piece joins its predecessor rather than standing alone.
    if pieces.len() >= 2
    {
        let last = pieces.last().unwrap();
        if last.end - last.start < MIN_PIECE
        {
            let last = pieces.pop().unwrap();
            let p = pieces.last_mut().unwrap();
            p.end = last.end;
            for i in 0..5
            {
                p.bytes[i] += last.bytes[i];
                if last.bytes[i] > 0
                {
                    p.label[i] = last.label[i];
                }
            }
        }
    }
    pieces
        .into_iter()
        .map(|p| {
            let dom = (0..5).max_by_key(|&i| p.bytes[i]).unwrap();
            (format!("0x{:x} {}", p.start, p.label[dom]), p.start, p.end)
        })
        .collect()
}

/// Build a package: one chunk for the whole file, or one per section.
fn buildPackage(
    data: &[u8],
    args: &Args,
    params: &codecs::Params,
    per_section: bool,
    portable_only: bool,
) -> Result<pack::Package, String>
{
    let jobs = sweepJobs();

    if !per_section
    {
        let (codec, packed) = bestFor(data, &jobs, params, portable_only, args.pick())
            .ok_or("no codec compressed this input")?;
        return Ok(pack::Package {
            chunks: vec![pack::Chunk {
                label: "whole file".into(),
                codec,
                offset: 0,
                original_len: data.len(),
                data: packed,
            }],
            original_len: data.len(),
            gaps: Vec::new(),
        });
    }

    // Pieces: the section table when there is one, else the entropy map's regions.
    let pieces: Vec<(String, usize, usize)> = match parseFiltered(data, args)
    {
        Some(c) if !c.sections.is_empty() => c
            .sections
            .iter()
            .filter(|s| !s.nobits)
            .map(|s| {
                (
                    s.name.clone(),
                    s.offset as usize,
                    (s.offset as usize + s.size as usize).min(data.len()),
                )
            })
            .collect(),
        Some(_) => return Err(format!("no section matched --section {:?}", args.section)),
        None =>
        {
            if !args.section.is_empty()
            {
                return Err(
                    "no section table to select from; --section needs ELF, PE, COFF, Mach-O, Wasm, or XCOFF"
                        .into(),
                );
            }
            let w = args.mapWindow(data.len());
            let map = emap::compute(data, w, w);
            piecesFromMap(&map, data.len())
        }
    };

    let pieces = coalesceSmall(withHoles(pieces, data.len()), MIN_CHUNK);

    let mut covered = vec![false; data.len()];
    let mut chunks = Vec::new();
    for (label, start, end) in pieces
    {
        if start >= end || covered[start]
        {
            continue;
        }
        let body = &data[start..end];
        if let Some((codec, packed)) = bestFor(body, &jobs, params, portable_only, args.pick())
        {
            chunks.push(pack::Chunk {
                label,
                codec,
                offset: start,
                original_len: body.len(),
                data: packed,
            });
            for c in &mut covered[start..end]
            {
                *c = true;
            }
        }
    }

    // Uncovered bytes (headers, padding, sections nothing shrank) get the same
    // sweep; only what still refuses to shrink is stored verbatim.
    let mut gaps = Vec::new();
    let mut i = 0usize;
    while i < data.len()
    {
        if covered[i]
        {
            i += 1;
            continue;
        }
        let start = i;
        while i < data.len() && !covered[i]
        {
            i += 1
        }
        let body = &data[start..i];
        match bestFor(body, &jobs, params, portable_only, args.pick())
        {
            Some((codec, packed)) => chunks.push(pack::Chunk {
                label: format!("gap 0x{:x}", start),
                codec,
                offset: start,
                original_len: body.len(),
                data: packed,
            }),
            None => gaps.push((start, body.to_vec())),
        }
    }
    chunks.sort_by_key(|c| c.offset);

    Ok(pack::Package {
        chunks,
        original_len: data.len(),
        gaps,
    })
}

/// Sort pieces and fill the holes between them with gap pieces so the file is tiled.
fn withHoles(mut pieces: Vec<(String, usize, usize)>, len: usize) -> Vec<(String, usize, usize)>
{
    pieces.sort_by_key(|p| p.1);
    let mut out = Vec::with_capacity(pieces.len() * 2);
    let mut pos = 0usize;
    for (label, start, end) in pieces
    {
        if start > pos
        {
            out.push((format!("gap 0x{:x}", pos), pos, start));
        }
        if end > start.max(pos)
        {
            out.push((label, start.max(pos), end));
        }
        pos = pos.max(end);
    }
    if pos < len
    {
        out.push((format!("gap 0x{:x}", pos), pos, len));
    }
    out
}

/// Pieces shorter than this are folded into a neighbour: a chunk header costs more than they save.
const MIN_CHUNK: usize = 64;

/// Fold pieces shorter than `min` into the adjacent piece after (or, for the last, before) them.
fn coalesceSmall(pieces: Vec<(String, usize, usize)>, min: usize) -> Vec<(String, usize, usize)>
{
    let mut out: Vec<(String, usize, usize)> = Vec::with_capacity(pieces.len());
    let mut pending: Option<(String, usize, usize)> = None;
    for (label, start, end) in pieces
    {
        let (label, start) = match pending.take()
        {
            Some((pl, ps, pe)) if pe == start => (joinLabel(&pl, &label), ps),
            Some(p) =>
            {
                out.push(p);
                (label, start)
            }
            None => (label, start),
        };
        if end - start < min
        {
            pending = Some((label, start, end));
        }
        else
        {
            out.push((label, start, end));
        }
    }
    if let Some((label, start, end)) = pending
    {
        match out.last_mut()
        {
            Some(last) if last.2 == start =>
            {
                last.0 = joinLabel(&last.0, &label);
                last.2 = end;
            }
            _ => out.push((label, start, end)),
        }
    }
    out
}

fn runUnpack(epk: &std::path::Path, args: &Args, st: &Style) -> std::process::ExitCode
{
    let data = match std::fs::read(epk)
    {
        Ok(d) => d,
        Err(e) =>
        {
            eprintln!("sprism: {}: {}", safePath(epk), e);
            return std::process::ExitCode::from(2);
        }
    };
    let params = codecs::Params::default();
    let out = match pack::deserialise(&data, &params)
    {
        Ok(o) => o,
        Err(e) =>
        {
            eprintln!("sprism: {}", e);
            return std::process::ExitCode::from(1);
        }
    };
    match &args.out
    {
        Some(p) => match std::fs::write(p, &out)
        {
            Ok(()) =>
            {
                eprintln!(
                    "{}",
                    st.dim(&format!(
                        "wrote {} bytes to {}",
                        commas(out.len()),
                        safePath(p)
                    ))
                );
            }
            Err(e) =>
            {
                eprintln!("sprism: {}", e);
                return std::process::ExitCode::from(1);
            }
        },
        None =>
        {
            use std::io::Write;
            let _ = std::io::stdout().write_all(&out);
        }
    }
    std::process::ExitCode::SUCCESS
}

fn runPack(path: &std::path::Path, data: &[u8], args: &Args, st: &Style) -> std::process::ExitCode
{
    let params = codecs::Params::default();
    let portable = args.emit_c;

    let per_section = args.sections || !args.section.is_empty();
    let pkg = match buildPackage(data, args, &params, per_section, portable)
    {
        Ok(p) => p,
        Err(e) =>
        {
            eprintln!("sprism: {}", e);
            return std::process::ExitCode::from(1);
        }
    };
    // Never write an artifact that has not been shown to reconstruct exactly.
    if let Err(e) = pack::verify(&pkg, data, &params)
    {
        eprintln!("sprism: packing failed verification: {}", e);
        return std::process::ExitCode::from(1);
    }

    let packed = pkg.packedLen();

    if args.json
    {
        let layout = pkg.layout();
        let chunks: Vec<String> = pkg.chunks.iter().zip(layout.iter()).map(|(c, off)| format!(
            "{{\"label\":{},\"codec\":{},\"decoder\":{},\"offset\":{},\"original_len\":{},\"packed_offset\":{},\"packed_len\":{},\"ratio\":{:.4}}}",
            jsonStr(&c.label), jsonStr(&c.codec), jsonStr(&pack::decoderHint(&c.codec)),
            c.offset, c.original_len, off, c.data.len(),
            c.data.len() as f64 / c.original_len.max(1) as f64)).collect();
        let gaps: Vec<String> = pkg
            .gaps
            .iter()
            .map(|(o, g)| format!("{{\"offset\":{},\"len\":{}}}", o, g.len()))
            .collect();
        println!("{{\"path\":{},\"original_len\":{},\"packed_len\":{},\"ratio\":{:.4},\"portable_only\":{},\"chunks\":[{}],\"verbatim\":[{}]}}",
                 jsonStr(&safePath(path)), data.len(), packed,
                 packed as f64 / data.len() as f64, portable, chunks.join(","), gaps.join(","));
    }

    if !args.json
    {
        println!("{}", st.bold(&safePath(path)));
        println!(
            "{} bytes -> {} bytes ({:.3}){}{}",
            commas(data.len()),
            commas(packed),
            packed as f64 / data.len() as f64,
            if portable
            {
                "  [dependency-free codecs only]"
            }
            else
            {
                ""
            },
            if args.pick() != Pick::Size
            {
                format!("  [codec per piece: {}]", args.pick().label())
            }
            else
            {
                String::new()
            }
        );
        if portable
        {
            let jobs = sweepJobs();
            if let Some((codec, best)) = bestFor(data, &jobs, &params, false, args.pick())
            {
                if best.len() < packed
                {
                    println!(
                        "{}",
                        st.dim(&format!(
                            "  {} on the whole file: {} ({:.3}), but it needs a library to unpack",
                            codec,
                            commas(best.len()),
                            best.len() as f64 / data.len() as f64
                        ))
                    );
                }
            }
        }
        if per_section
        {
            println!(
                "{}",
                st.dim(&format!(
                    "{} chunk(s), {} verbatim region(s)",
                    pkg.chunks.len(),
                    pkg.gaps.len()
                ))
            );
            let layout = pkg.layout();
            let lw = pkg
                .chunks
                .iter()
                .map(|c| c.label.chars().count())
                .max()
                .unwrap_or(0)
                .clamp(12, 32);
            println!(
                "  {} {} {} {} {} {}",
                pad(&st.bold("piece"), lw, false),
                pad(&st.bold("at"), 10, true),
                pad(&st.bold("bytes"), 10, true),
                pad(&st.bold("packed"), 10, true),
                pad(&st.bold("epk@"), 10, true),
                st.bold("codec")
            );
            let limit = if args.limit == 0
            {
                pkg.chunks.len()
            }
            else
            {
                args.limit.min(pkg.chunks.len())
            };
            for (c, off) in pkg.chunks.iter().zip(layout.iter()).take(limit)
            {
                println!(
                    "  {} {} {} {} {} {}",
                    pad(&trunc(&c.label, lw), lw, false),
                    pad(&format!("0x{:x}", c.offset), 10, true),
                    pad(&commas(c.original_len), 10, true),
                    pad(&commas(c.data.len()), 10, true),
                    pad(&format!("0x{:x}", off), 10, true),
                    st.cyan(&c.codec)
                );
            }
            if pkg.chunks.len() > limit
            {
                println!(
                    "  {}",
                    st.dim(&format!(
                        "({} more; --limit 0 for all)",
                        pkg.chunks.len() - limit
                    ))
                );
            }
            let mut codecs_used: Vec<&str> = pkg.chunks.iter().map(|c| c.codec.as_str()).collect();
            codecs_used.sort_unstable();
            codecs_used.dedup();
            let decoders: Vec<String> = codecs_used
                .iter()
                .map(|c| format!("{} = {}", c, pack::decoderHint(c)))
                .collect();
            println!(
                "  {}",
                st.dim(&format!("decoders: {}", decoders.join("; ")))
            );
            let verbatim: usize = pkg.gaps.iter().map(|(_, g)| g.len()).sum();
            if verbatim > 0
            {
                println!("  {}", st.dim(&format!(
                "{} bytes in {} region(s) stored verbatim: headers, padding, and pieces nothing shrank",
                commas(verbatim), pkg.gaps.len())));
            }
        }
        else
        {
            println!(
                "{}",
                st.dim(&format!(
                    "codec: {} ({})",
                    pkg.chunks[0].codec,
                    pack::decoderHint(&pkg.chunks[0].codec)
                ))
            );
        }
        println!(
            "{}",
            st.good("verified: unpacking reproduces the input exactly")
        );
    } // !json

    if args.pack && !args.emit_c
    {
        let out = args.out.clone().unwrap_or_else(|| {
            let mut p = path.to_path_buf();
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "output".into());
            p.set_file_name(format!("{}.epk", name));
            p
        });
        match pack::serialise(&pkg).and_then(|b| std::fs::write(&out, b).map_err(|e| e.to_string()))
        {
            Ok(()) =>
            {
                if !args.json
                {
                    println!("wrote {}", safePath(&out))
                }
            }
            Err(e) =>
            {
                eprintln!("sprism: {}", e);
                return std::process::ExitCode::from(1);
            }
        }
    }
    if args.emit_c
    {
        let c_out = args.out.clone().unwrap_or_else(|| {
            let mut p = path.to_path_buf();
            let name = p
                .file_stem()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "output".into());
            p.set_file_name(format!("{}_unpack.c", name));
            p
        });
        let epk_out = if args.out.is_some()
        {
            c_out.with_extension("epk")
        }
        else
        {
            let mut p = path.to_path_buf();
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "output".into());
            p.set_file_name(format!("{}.epk", name));
            p
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "input".into());
        let generated = pack::serialise(&pkg)
            .and_then(|epk| pack::emitC(&pkg, &params, &name).map(|c| (epk, c)))
            .and_then(|(epk, c)| {
                std::fs::write(&epk_out, epk).map_err(|e| e.to_string())?;
                std::fs::write(&c_out, c).map_err(|e| e.to_string())
            });
        match generated
        {
            Ok(()) =>
            {
                println!("wrote {}", safePath(&c_out));
                println!("wrote {}", safePath(&epk_out));
                println!(
                    "{}",
                    st.dim(
                        "  sprism_unpack(epk, epk_len, out, out_len) -> bytes written, 0 on failure"
                    )
                );
            }
            Err(e) =>
            {
                eprintln!("sprism: {}", e);
                return std::process::ExitCode::from(1);
            }
        }
    }
    std::process::ExitCode::SUCCESS
}

// ---------------------------------------------------------------------------
// --carve: embedded streams
// ---------------------------------------------------------------------------

fn carveJson(f: &carve::Found, base: usize) -> String
{
    let kids: Vec<String> = f.children.iter().map(|k| carveJson(k, 0)).collect();
    format!(
        "{{\"offset\":{},\"file_offset\":{},\"codec\":{},\"by_magic\":{},\"stream_len\":{},\"decoded_len\":{},\"capped\":{},\"partial\":{},\"entropy\":{:.4},\"band\":{},\"chi2\":{},\"chi2_p\":{},\"uniform\":{},\"identified\":{},\"container\":{},\"children\":[{}]}}",
        f.offset, base + f.offset, jsonStr(f.codec), f.by_magic, f.stream_len, f.decoded_len,
        f.capped,
        f.partial.as_ref().map(|e| jsonStr(e)).unwrap_or_else(|| "null".into()),
        f.entropy,
        f.band.map(|b| jsonStr(b.describe(f.uniformity))).unwrap_or_else(|| "null".into()),
        f.uniformity.map(|u| format!("{:.2}", u.chi2)).unwrap_or_else(|| "null".into()),
        f.uniformity.map(|u| format!("{:.4e}", u.p)).unwrap_or_else(|| "null".into()),
        f.uniformity.map(|u| u.isUniform().to_string()).unwrap_or_else(|| "null".into()),
        jsonStr(&f.identity),
        f.container.as_ref().map(|c| jsonStr(c)).unwrap_or_else(|| "null".into()),
        kids.join(",")
    )
}

fn printFound(f: &carve::Found, st: &Style, indent: usize)
{
    let ind = "  ".repeat(indent);
    let what = match &f.container
    {
        Some(c) => st.bold(c),
        None => f.identity.clone(),
    };
    let band = match f.band
    {
        Some(b) => bandColor(st, b, b.describe(f.uniformity)),
        None => st.dim("-"),
    };
    let how = if f.by_magic
    {
        ""
    }
    else
    {
        " (probed, no magic)"
    };
    let size = format!(
        "{}{}",
        commas(f.decoded_len),
        if f.capped { "+" } else { "" }
    );
    println!(
        "{}{} {} {} -> {} bytes   H {:.2} {}   {}{}",
        ind,
        st.bold(&format!("0x{:x}", f.offset)),
        pad(&st.cyan(f.codec), 8, false),
        pad(&commas(f.stream_len), 11, true),
        size,
        f.entropy,
        band,
        what,
        st.dim(how)
    );
    if let Some(u) = f.uniformity
    {
        println!(
            "{}{}",
            ind,
            st.dim(&format!("         payload histogram: {}", u.summary()))
        );
    }
    if f.capped
    {
        println!(
            "{}{}",
            ind,
            st.warn(&format!(
                "         output cut at the {} decode cap; the real payload is larger",
                carve::human(f.decoded_len)
            ))
        );
    }
    if let Some(e) = &f.partial
    {
        println!(
            "{}{}",
            ind,
            st.warn(&format!(
                "         decoder stopped after {} input bytes: {} (truncated or corrupt stream)",
                commas(f.stream_len),
                e
            ))
        );
    }
    for k in &f.children
    {
        printFound(k, st, indent + 2);
    }
}

fn runCarve(
    path: &std::path::Path,
    data: &[u8],
    args: &Args,
    st: &Style,
    overview: bool,
) -> std::process::ExitCode
{
    let opts = carve::Options::default();
    let map_window = args.mapWindow(data.len());
    let smap = emap::compute(data, map_window, map_window.max(1));

    let ranges = carve::scanRanges(&smap, data.len());
    let scanned: usize = ranges.iter().map(|(a, b)| b - a).sum();
    let found = carve::carve(data, &smap, &opts);

    if overview && !args.json
    {
        let ident = format::identify(data);
        println!("{}", st.bold(&safePath(path)));
        println!(
            "{} bytes   H {:.3}   identified as: {}",
            commas(data.len()),
            Metrics::of(data).entropy,
            ident.label
        );
        printMapOverview(data, &smap, st, &found);
    }

    if args.json
    {
        let items: Vec<String> = found.iter().map(|f| carveJson(f, 0)).collect();
        println!("{{\"path\":{},\"size\":{},\"scanned_bytes\":{},\"regions_scanned\":{},\"streams\":[{}]}}",
                 jsonStr(&safePath(path)), data.len(), scanned, ranges.len(), items.join(","));
    }
    else
    {
        println!();
        println!(
            "{}",
            st.bold(&format!(
                "carve: {} stream(s) in {} high-entropy range(s), {} bytes scanned",
                found.len(),
                ranges.len(),
                commas(scanned)
            ))
        );
        if found.is_empty()
        {
            if ranges.is_empty()
            {
                println!(
                    "  {}",
                    st.dim("no compressed or random regions in the map: nothing to carve")
                );
            }
            else
            {
                println!("  {}", st.dim(
                    "no decodable stream header in the hot regions: the data is encrypted, \
                     uses a codec without a magic number, or is compressed by something not linked here"));
            }
        }
        else
        {
            println!(
                "  {}",
                st.dim("offset   codec       stream -> decoded          payload")
            );
            for f in &found
            {
                printFound(f, st, 1);
            }
            let nested: usize = found.iter().map(|f| f.children.len()).sum();
            if nested > 0
            {
                println!("  {}", st.dim(&format!(
                    "{} nested stream(s) found one level down; --carve the written payload to go deeper",
                    nested)));
            }
        }
    }

    if let Some(dir) = &args.out
    {
        if !found.is_empty()
        {
            if let Err(e) = std::fs::create_dir_all(dir)
            {
                eprintln!("sprism: {}: {}", safePath(dir), e);
                return std::process::ExitCode::from(1);
            }
            let mut written = 0usize;
            for f in &found
            {
                let name = format!("{:08x}_{}.bin", f.offset, f.codec);
                let out = dir.join(&name);
                match std::fs::write(&out, &f.payload)
                {
                    Ok(()) => written += 1,
                    Err(e) => eprintln!("sprism: {}: {}", safePath(&out), e),
                }
            }
            if !args.json
            {
                println!(
                    "{}",
                    st.dim(&format!(
                        "wrote {} payload(s) to {}",
                        written,
                        safePath(dir)
                    ))
                );
            }
        }
    }
    std::process::ExitCode::SUCCESS
}

// ---------------------------------------------------------------------------
// --at: zoom in on one region
// ---------------------------------------------------------------------------

fn parseOffset(spec: &str) -> Option<usize>
{
    let t = spec.trim();
    if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X"))
    {
        usize::from_str_radix(h, 16).ok()
    }
    else
    {
        t.parse().ok()
    }
}

/// Drill into a range: per-window entropy, hex dump, enclosing section, and what packs it.
fn runAt(
    path: &std::path::Path,
    data: &[u8],
    spec: &str,
    args: &Args,
    st: &Style,
) -> std::process::ExitCode
{
    let Some(start) = parseOffset(spec)
    else
    {
        eprintln!("sprism: --at {}: expected a decimal or 0x offset", spec);
        return std::process::ExitCode::from(2);
    };
    if start >= data.len()
    {
        eprintln!(
            "sprism: --at 0x{:x} is past the end ({} bytes)",
            start,
            commas(data.len())
        );
        return std::process::ExitCode::from(2);
    }
    let end = (start + args.len).min(data.len());
    let slice = &data[start..end];
    let m = Metrics::of(slice);

    let container = format::parse(data);
    let section = container.as_ref().and_then(|c| {
        c.sections.iter().find(|s| {
            !s.nobits && start >= s.offset as usize && start < (s.offset + s.size) as usize
        })
    });
    let function = container.as_ref().and_then(|c| {
        c.functions.iter().find(|f| {
            f.size > 0
                && f.file_offset.is_some_and(|off| {
                    start as u64 >= off && (start as u64) < off.saturating_add(f.size)
                })
        })
    });

    println!("{}", st.bold(&safePath(path)));
    println!(
        "0x{:x}-0x{:x}   {} bytes   H {:.3}   {} distinct{}{}",
        start,
        end,
        commas(slice.len()),
        m.entropy,
        m.unique,
        section
            .map(|s| format!("   in {}", s.name))
            .unwrap_or_default(),
        function
            .map(|f| format!("   function {}", f.name))
            .unwrap_or_default()
    );

    // Below 1 KB a windowed profile measures sample size, not the file.
    if slice.len() >= 1024
    {
        let win = (slice.len() / 32).clamp(128, 4096);
        let sub = emap::compute(slice, win, win);
        let cells = emap::strip(&sub, 64.min(sub.blocks.len().max(1)));
        let strip: String = cells
            .iter()
            .map(|b| bandColor(st, *b, &b.glyph().to_string()))
            .collect();
        println!("  {}", strip);
        println!(
            "{}",
            st.dim(&format!(
                "  window {} over {} blocks",
                win,
                sub.blocks.len()
            ))
        );
    }
    else
    {
        println!(
            "{}",
            st.dim("  (range too short for an entropy profile; raise --len past 1024)")
        );
    }

    // Hex dump, capped so a large --len stays readable.
    println!();
    let show = slice.len().min(512);
    for (i, row) in slice[..show].chunks(16).enumerate()
    {
        let off = start + i * 16;
        let hex: String = row.iter().map(|b| format!("{:02x} ", b)).collect();
        let ascii: String = row
            .iter()
            .map(|&b| {
                if (32..127).contains(&b)
                {
                    b as char
                }
                else
                {
                    '.'
                }
            })
            .collect();
        println!(
            "  {} {} {}",
            st.dim(&format!("{:08x}", off)),
            pad(&hex, 48, false),
            st.dim(&ascii)
        );
    }
    if slice.len() > show
    {
        println!(
            "{}",
            st.dim(&format!(
                "  ... {} more bytes (raise --len to widen, it is the dump that is capped)",
                commas(slice.len() - show)
            ))
        );
    }

    // Probe every carve codec at this one offset, headerless ones included.
    let carved: Vec<carve::Found> = carve::CODECS
        .iter()
        .filter_map(|c| carve::probeAt(data, start, c, carve::Options::default().max_decoded))
        .collect();
    if !carved.is_empty()
    {
        println!();
        println!("{}", st.bold("a stream starts here:"));
        for f in &carved
        {
            printFound(f, st, 1);
        }
        println!("{}", st.dim("  (--carve walks every high-entropy region for these; --out DIR writes the payloads)"));
    }

    let params = codecs::Params::default();
    let jobs = sweepJobs();
    println!();
    match sweepBest(
        slice,
        &jobs,
        &params,
        true,
        args.targetLow(),
        args.targetHigh(),
        args.pick(),
    )
    {
        Some(f) if f.codec != "(none)" =>
        {
            println!(
                "best codec here: {} -> {} bytes ({:.3}), packed H {:.3}",
                f.codec,
                commas(f.size),
                f.ratio,
                f.entropy
            );
            if let Some((c, sz, r)) = &f.in_band
            {
                println!(
                    "{}",
                    st.dim(&format!(
                        "  staying inside [{:.1}, {:.1}]: {} -> {} ({:.3})",
                        args.targetLow(),
                        args.targetHigh(),
                        c,
                        commas(*sz),
                        r
                    ))
                );
            }
        }
        _ => println!(
            "{}",
            st.warn("nothing compresses this range: already packed, encrypted, or too small")
        ),
    }
    std::process::ExitCode::SUCCESS
}

fn trunc(s: &str, w: usize) -> String
{
    if s.chars().count() <= w
    {
        s.to_string()
    }
    else
    {
        let keep: String = s.chars().take(w - 1).collect();
        format!("{}~", keep)
    }
}

/// Label for two merged pieces; a gap name is dropped when a section name is present.
fn joinLabel(a: &str, b: &str) -> String
{
    let gap = |l: &str| l.starts_with("gap ");
    let with_gap = |l: &str| {
        if l.ends_with("+gap")
        {
            l.to_string()
        }
        else
        {
            format!("{}+gap", l)
        }
    };
    match (gap(a), gap(b))
    {
        (true, true) => a.to_string(),
        (true, false) => format!("gap+{}", b),
        (false, true) => with_gap(a),
        (false, false) => format!("{}+{}", a, b),
    }
}

// ---------------------------------------------------------------------------
// Per-chunk sweeps, for --sections, --pack and --at
// ---------------------------------------------------------------------------

/// Best result found for one chunk of a file.
struct BestFit
{
    codec: String,
    size: usize,
    ratio: f64,
    entropy: f64,
    /// Best codec whose output also lands inside the entropy band, if any.
    in_band: Option<(String, usize, f64)>,
}

/// Chunks below this are not swept; kept tiny because 95 repeated bytes still pack to 9.
const MIN_SWEEPABLE: usize = 8;

/// Run the codec set over one chunk and return the best verified result.
fn sweepBest(
    data: &[u8],
    jobs: &[Job],
    params: &codecs::Params,
    verify: bool,
    target_low: f64,
    target_high: f64,
    pick: Pick,
) -> Option<BestFit>
{
    if data.len() < MIN_SWEEPABLE
    {
        return None;
    }
    let original = Metrics::of(data);

    let rows: Vec<Row> = jobs
        .par_iter()
        .map(|job| {
            let (out, elapsed, ok, err) = runJob(job, data, params, verify);
            score::score(
                job.name.clone(),
                out,
                elapsed,
                ok,
                err,
                &original,
                target_high,
            )
        })
        .collect();

    let usable: Vec<&Row> = rows
        .iter()
        .filter(|r| r.verdict != Verdict::Broken && r.name != "raw" && r.metrics.size > 0)
        .collect();

    // Size-preserving transforms all tie at ratio 1.000 on an incompressible
    // chunk; report "(none)" rather than an arbitrary winner.
    let shrinking: Vec<&&Row> = usable.iter().filter(|r| r.ratio < 1.0).collect();
    if shrinking.is_empty()
    {
        let floor = usable
            .iter()
            .min_by(|a, b| a.ratio.partial_cmp(&b.ratio).unwrap())?;
        return Some(BestFit {
            codec: "(none)".into(),
            size: data.len(),
            ratio: 1.0,
            entropy: floor.metrics.entropy,
            in_band: None,
        });
    }
    let best = shrinking.iter().copied().reduce(|inc, cand| {
        if pick.better(
            (cand.metrics.size, cand.metrics.entropy),
            (inc.metrics.size, inc.metrics.entropy),
        )
        {
            cand
        }
        else
        {
            inc
        }
    })?;

    let in_band = usable
        .iter()
        .filter(|r| {
            r.metrics.entropy <= target_high && r.metrics.entropy >= target_low && r.ratio < 1.0
        })
        .min_by(|a, b| a.ratio.partial_cmp(&b.ratio).unwrap())
        .map(|r| (r.name.clone(), r.metrics.size, r.ratio));

    Some(BestFit {
        codec: best.name.clone(),
        size: best.metrics.size,
        ratio: best.ratio,
        entropy: best.metrics.entropy,
        in_band,
    })
}

/// Codec set for per-chunk sweeps: the slowest codecs are dropped.
fn sweepJobs() -> Vec<Job>
{
    buildJobs()
        .into_iter()
        .filter(|j| {
            !registry::find(&j.name)
                .map(|c| c.slow_for_chunks)
                .unwrap_or(false)
        })
        .collect()
}

fn main() -> std::process::ExitCode
{
    use std::io::IsTerminal;

    let args = Args::parse();
    let st = Style {
        on: std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal(),
    };

    if let Some(epk) = &args.unpack
    {
        return runUnpack(epk, &args, &st);
    }

    if args.pick() != Pick::Size
        && !(args.sections
            || args.pack
            || args.emit_c
            || args.at.is_some()
            || !args.section.is_empty())
    {
        eprintln!("sprism: --pick applies to --sections, --pack, --emit-c and --at");
        return std::process::ExitCode::from(2);
    }

    if args.list
    {
        println!(
            "{}",
            st.bold("codecs ([C] has a dependency-free C decoder):")
        );
        let available: Vec<&str> = libs::available().iter().map(|c| c.name).collect();
        let mut families = Vec::new();
        for codec in registry::ALL
        {
            if !families.contains(&codec.family)
            {
                families.push(codec.family);
            }
        }
        for family in families
        {
            print!("  {:14}", registry::familyLabel(family));
            for codec in registry::ALL.iter().filter(|c| c.family == family)
            {
                if codec.backend == registry::Backend::Library && !available.contains(&codec.name)
                {
                    continue;
                }
                print!(
                    " {}{}",
                    codec.name,
                    if codec.c_decoder.is_some() { "[C]" } else { "" }
                );
            }
            println!();
        }
        let missing = libs::unavailableNames();
        if !missing.is_empty()
        {
            println!(
                "{}",
                st.dim(&format!(
                    "\nnot supported by the linked liblzma: {}",
                    missing.join(" ")
                ))
            );
        }
        return std::process::ExitCode::SUCCESS;
    }

    // A directory means corpus mode.
    if let Some(p) = &args.file
    {
        if p.is_dir()
        {
            return runCorpus(p, &args, &st);
        }
    }

    let path = match args.file.clone()
    {
        Some(p) => p,
        None =>
        {
            eprintln!("sprism: a file or directory is required");
            return std::process::ExitCode::from(2);
        }
    };
    let data = match std::fs::read(&path)
    {
        Ok(d) => d,
        Err(e) =>
        {
            eprintln!("sprism: {}: {}", path.display(), e);
            return std::process::ExitCode::from(2);
        }
    };
    if data.is_empty()
    {
        eprintln!("sprism: {}: empty file", path.display());
        return std::process::ExitCode::from(2);
    }

    if let Some(spec) = &args.at
    {
        return runAt(&path, &data, spec, &args, &st);
    }
    if args.pack || args.emit_c
    {
        return runPack(&path, &data, &args, &st);
    }
    // --carve on its own; with --sections it runs after the section report.
    // The JSON form is one document per mode, so --carve --json is the carve
    // document whatever else was asked for.
    if args.carve && (args.json || (!args.sections && args.section.is_empty()))
    {
        return runCarve(&path, &data, &args, &st, true);
    }

    if args.list_sections
    {
        return runListSections(&path, &data, &st);
    }
    if args.sections || !args.section.is_empty()
    {
        return runSections(&path, &data, &args, &st);
    }

    // The whole-file sweep holds many buffers at once; the cheap modes ran already.
    if args.max_size > 0 && data.len() > args.max_size * 1024 * 1024
    {
        eprintln!("sprism: {}: {} exceeds --max-size {} MB. Use --sections or --map for large files, or raise --max-size.",
                  safePath(&path), commas(data.len()), args.max_size);
        return std::process::ExitCode::from(2);
    }

    if let Some(n) = args.threads
    {
        let _ = rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .build_global();
    }
    else if data.len() > 2 * 1024 * 1024
    {
        // Each worker holds a compressor's working set plus its output, so
        // fewer threads as the input grows keeps peak memory bounded.
        let budget = 256 * 1024 * 1024;
        let available = std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1);
        let cap = (budget / data.len()).clamp(1, available);
        let _ = rayon::ThreadPoolBuilder::new()
            .num_threads(cap)
            .build_global();
    }

    let params = codecs::Params::default();
    let original = Metrics::of(&data);

    {
        println!("{}", st.bold(&safePath(&path)));
        println!(
            "{} bytes   H {:.3} bits/byte   {} distinct values   {} bytes order-0 estimate",
            commas(data.len()),
            original.entropy,
            original.unique,
            commas((original.infoBits() / 8.0) as usize)
        );
        let ident = format::identify(&data);
        println!("{}", st.dim(&format!("identified as: {}", ident.label)));
        if ident.precompressed
        {
            println!(
                "{}",
                st.warn("already a compressed container; every codec will expand it")
            );
        }
        println!(
            "{}",
            st.dim(&format!(
                "entropy band [{:.1}, {:.1}]   verify {}   {} threads",
                args.targetLow(),
                args.targetHigh(),
                "on",
                rayon::current_num_threads()
            ))
        );
    }

    let mut jobs = buildJobs();
    let mut omitted = Vec::new();
    if !args.exhaustive
    {
        jobs.retain(|job| {
            let keep = registry::includeInDefaultWholeSweep(&job.name, data.len());
            if !keep
            {
                omitted.push(job.name.clone());
            }
            keep
        });
    }
    if !omitted.is_empty()
    {
        eprintln!(
            "  omitted for large input: {} (use --exhaustive to include)",
            omitted.join(", ")
        );
    }
    let t0 = Instant::now();
    let completed = AtomicUsize::new(0);
    let progress_lock = Mutex::new(());
    let total_jobs = jobs.len();

    let mut rows: Vec<Row> = jobs
        .par_iter()
        .map(|job| {
            let (out, elapsed, ok, err) = runJob(job, &data, &params, true);
            let row = score::score(
                job.name.clone(),
                out,
                elapsed,
                ok,
                err,
                &original,
                args.targetHigh(),
            );
            let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
            let _guard = progress_lock.lock().unwrap_or_else(|e| e.into_inner());
            let width = 30;
            let filled = width * done / total_jobs;
            eprint!(
                "\r  sweeping codecs [{}{}] {}/{}  last: {:<18}",
                "=".repeat(filled),
                " ".repeat(width - filled),
                done,
                total_jobs,
                job.name
            );
            let _ = std::io::stderr().flush();
            row
        })
        .collect();
    eprintln!();

    let wall = t0.elapsed().as_secs_f64();

    let key = match args.sort.as_str()
    {
        "info" => SortKey::Info,
        "ratio" => SortKey::Ratio,
        "size" => SortKey::Size,
        "entropy" => SortKey::Entropy,
        "time" => SortKey::Time,
        "name" => SortKey::Name,
        _ => SortKey::Score,
    };
    score::sortRows(&mut rows, key);

    let width: usize = COLS.iter().map(|c| c.1).sum::<usize>() + COLS.len() - 1;
    render(&rows, &st, &args, width);

    {
        println!();
        println!("{}", st.dim(&format!(
            "{} smaller, H in band   {} smaller, H above {:.1}   {} order-0 estimate unchanged   {} not smaller   {} failed round-trip",
            st.good("hit"), st.warn("over-H"), args.targetHigh(), st.odd("recode"), st.dim("expand"), st.bad("BROKEN"))));
        println!(
            "{}",
            st.dim("infoR = size x H relative to the original; ranking key")
        );

        let broken: Vec<&Row> = rows
            .iter()
            .filter(|r| r.verdict == Verdict::Broken)
            .collect();
        if !broken.is_empty()
        {
            println!();
            println!(
                "{}",
                st.bad(&format!("failed round-trip ({}):", broken.len()))
            );
            for r in &broken
            {
                println!("  {}: {}", r.name, r.note);
            }
        }

        println!();
        let best = rows
            .iter()
            .filter(|r| r.verdict == Verdict::Hit)
            .min_by(|a, b| a.info_ratio.partial_cmp(&b.info_ratio).unwrap());
        match best
        {
            Some(b) =>
            {
                println!("{}", st.bold("best within the entropy band:"));
                println!(
                    "  {}: {} bytes, ratio {:.3}, infoR {:.3}, H {:.3}",
                    b.name,
                    commas(b.metrics.size),
                    b.ratio,
                    b.info_ratio,
                    b.metrics.entropy
                );
            }
            None =>
            {
                println!(
                    "{}",
                    st.warn("nothing landed inside the entropy band while reducing information.")
                );
                if let Some(b) = rows
                    .iter()
                    .filter(|r| r.verdict == Verdict::OverH)
                    .min_by(|a, b| a.info_ratio.partial_cmp(&b.info_ratio).unwrap())
                {
                    println!(
                        "  smallest was {} at infoR {:.3}, but H {:.3} exceeds the {:.1} ceiling.",
                        b.name,
                        b.info_ratio,
                        b.metrics.entropy,
                        args.targetHigh()
                    );
                }
            }
        }
        println!();
        println!(
            "{}",
            st.dim(&format!(
                "{} codecs in {:.2}s",
                jobs.len().saturating_sub(1),
                wall
            ))
        );
    }

    std::process::ExitCode::SUCCESS
}
