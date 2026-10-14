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

//! Native transforms and LZ codecs. Every encoder has a matching decoder.

const HASH_BITS: u32 = 17;
const HASH_SIZE: usize = 1 << HASH_BITS;
const GOLDEN: u32 = 0x9E37_79B1;

// ---------------------------------------------------------------------------
// Params
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct Params
{
    pub min_run: usize,
    pub min_match: usize,
    pub max_match: usize,
    pub window: usize,
    pub max_chain: usize,
    pub esc: u8,
    pub bpe_rules: usize,
}

impl Default for Params
{
    fn default() -> Self
    {
        Params {
            min_run: 4,
            min_match: 3,
            max_match: 64,
            window: 8192,
            max_chain: 64,
            esc: 0x90,
            bpe_rules: 64,
        }
    }
}

// ---------------------------------------------------------------------------
// Metrics
// ---------------------------------------------------------------------------

fn histogram(data: &[u8]) -> [u64; 256]
{
    // Four interleaved accumulators break the store-to-load dependency chain.
    let mut h0 = [0u64; 256];
    let mut h1 = [0u64; 256];
    let mut h2 = [0u64; 256];
    let mut h3 = [0u64; 256];
    let mut chunks = data.chunks_exact(4);
    for c in &mut chunks
    {
        h0[c[0] as usize] += 1;
        h1[c[1] as usize] += 1;
        h2[c[2] as usize] += 1;
        h3[c[3] as usize] += 1;
    }
    for &b in chunks.remainder()
    {
        h0[b as usize] += 1;
    }
    for i in 0..256
    {
        h0[i] += h1[i] + h2[i] + h3[i];
    }
    h0
}

/// (entropy_bits_per_byte, printable_ratio, ascii_ratio, nul_ratio, unique_bytes)
pub fn metricsOf(data: &[u8]) -> (f64, f64, f64, f64, usize)
{
    if data.is_empty()
    {
        return (0.0, 0.0, 0.0, 0.0, 0);
    }
    let hist = histogram(data);
    let n = data.len() as f64;
    let mut ent = 0.0f64;
    let mut printable = 0u64;
    let mut ascii = 0u64;
    let mut unique = 0usize;
    for (b, &c) in hist.iter().enumerate()
    {
        if c == 0
        {
            continue;
        }
        unique += 1;
        let p = c as f64 / n;
        ent -= p * p.log2();
        if b == 9 || b == 10 || b == 13 || (32..=126).contains(&b)
        {
            printable += c;
        }
        if b < 128
        {
            ascii += c;
        }
    }
    (
        ent,
        printable as f64 / n,
        ascii as f64 / n,
        hist[0] as f64 / n,
        unique,
    )
}

// ---------------------------------------------------------------------------
// Hash-chain match finder
// ---------------------------------------------------------------------------

struct MatchFinder<'a>
{
    data: &'a [u8],
    head: Vec<i32>,
    prev: Vec<i32>,
    hash_len: usize,
}

impl<'a> MatchFinder<'a>
{
    fn new(data: &'a [u8], hash_len: usize) -> Self
    {
        MatchFinder {
            data,
            head: vec![-1i32; HASH_SIZE],
            prev: vec![-1i32; data.len().max(1)],
            hash_len,
        }
    }

    #[inline]
    fn hashAt(&self, p: usize) -> usize
    {
        let d = self.data;
        let v = if self.hash_len == 4
        {
            u32::from_le_bytes([d[p], d[p + 1], d[p + 2], d[p + 3]])
        }
        else
        {
            (d[p] as u32) | ((d[p + 1] as u32) << 8) | ((d[p + 2] as u32) << 16)
        };
        (v.wrapping_mul(GOLDEN) >> (32 - HASH_BITS)) as usize
    }

    #[inline]
    fn insert(&mut self, p: usize)
    {
        if p + self.hash_len > self.data.len()
        {
            return;
        }
        let h = self.hashAt(p);
        self.prev[p] = self.head[h];
        self.head[h] = p as i32;
    }

    /// Longest match for position `p`. Returns (len, dist), len == 0 if none.
    fn find(
        &self,
        p: usize,
        window: usize,
        min_match: usize,
        max_match: usize,
        max_chain: usize,
    ) -> (usize, usize)
    {
        let d = self.data;
        let n = d.len();
        if p + self.hash_len > n || p + min_match > n
        {
            return (0, 0);
        }
        let limit = max_match.min(n - p);
        if limit < min_match
        {
            return (0, 0);
        }
        let mut best_len = 0usize;
        let mut best_dist = 0usize;
        let mut cand = self.head[self.hashAt(p)];
        let mut checked = 0usize;

        while cand >= 0 && checked < max_chain
        {
            let j = cand as usize;
            let dist = p - j;
            if dist == 0 || dist > window
            {
                break;
            }
            // Cheap reject: compare the byte that would extend the current best.
            if best_len == 0 || d[j + best_len] == d[p + best_len]
            {
                let mut l = 0usize;
                while l < limit && d[j + l] == d[p + l]
                {
                    l += 1;
                }
                if l >= min_match && l > best_len
                {
                    best_len = l;
                    best_dist = dist;
                    if best_len == limit
                    {
                        break;
                    }
                }
            }
            checked += 1;
            cand = self.prev[j];
        }
        (best_len, best_dist)
    }
}

// ---------------------------------------------------------------------------
// Byte-level transforms (encode + decode)
// ---------------------------------------------------------------------------

fn dpcmEnc(d: &[u8]) -> Vec<u8>
{
    let mut out = Vec::with_capacity(d.len());
    let mut prev = 0u8;
    for (i, &b) in d.iter().enumerate()
    {
        if i == 0
        {
            out.push(b);
        }
        else
        {
            out.push(b.wrapping_sub(prev));
        }
        prev = b;
    }
    out
}

fn dpcmDec(d: &[u8]) -> Vec<u8>
{
    let mut out = Vec::with_capacity(d.len());
    let mut prev = 0u8;
    for (i, &b) in d.iter().enumerate()
    {
        let v = if i == 0 { b } else { b.wrapping_add(prev) };
        out.push(v);
        prev = v;
    }
    out
}

fn xorEnc(d: &[u8]) -> Vec<u8>
{
    let mut out = Vec::with_capacity(d.len());
    let mut prev = 0u8;
    for (i, &b) in d.iter().enumerate()
    {
        if i == 0
        {
            out.push(b);
        }
        else
        {
            out.push(b ^ prev);
        }
        prev = b;
    }
    out
}

fn xorDec(d: &[u8]) -> Vec<u8>
{
    let mut out = Vec::with_capacity(d.len());
    let mut prev = 0u8;
    for (i, &b) in d.iter().enumerate()
    {
        let v = if i == 0 { b } else { b ^ prev };
        out.push(v);
        prev = v;
    }
    out
}

fn delta16Enc(d: &[u8]) -> Vec<u8>
{
    if d.len() < 2
    {
        return d.to_vec();
    }
    let mut out = Vec::with_capacity(d.len());
    out.extend_from_slice(&d[0..2]);
    let mut prev = u16::from_le_bytes([d[0], d[1]]);
    let mut i = 2;
    while i + 1 < d.len()
    {
        let cur = u16::from_le_bytes([d[i], d[i + 1]]);
        out.extend_from_slice(&cur.wrapping_sub(prev).to_le_bytes());
        prev = cur;
        i += 2;
    }
    if i < d.len()
    {
        out.push(d[i]);
    }
    out
}

fn delta16Dec(d: &[u8]) -> Vec<u8>
{
    if d.len() < 2
    {
        return d.to_vec();
    }
    let mut out = Vec::with_capacity(d.len());
    out.extend_from_slice(&d[0..2]);
    let mut prev = u16::from_le_bytes([d[0], d[1]]);
    let mut i = 2;
    while i + 1 < d.len()
    {
        let delta = u16::from_le_bytes([d[i], d[i + 1]]);
        let cur = prev.wrapping_add(delta);
        out.extend_from_slice(&cur.to_le_bytes());
        prev = cur;
        i += 2;
    }
    if i < d.len()
    {
        out.push(d[i]);
    }
    out
}

fn rlePairEnc(d: &[u8]) -> Vec<u8>
{
    if d.is_empty()
    {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut prev = d[0];
    let mut count: u32 = 1;
    for &b in &d[1..]
    {
        if b == prev && count < 255
        {
            count += 1;
        }
        else
        {
            out.push(count as u8);
            out.push(prev);
            prev = b;
            count = 1;
        }
    }
    out.push(count as u8);
    out.push(prev);
    out
}

fn rlePairDec(d: &[u8]) -> Result<Vec<u8>, String>
{
    if d.len() % 2 != 0
    {
        return Err("rle_pair: odd length".into());
    }
    let mut out = Vec::new();
    for c in d.chunks_exact(2)
    {
        out.extend(std::iter::repeat(c[1]).take(c[0] as usize));
    }
    Ok(out)
}

/// Escape RLE:  ESC 0 -> literal ESC;  ESC count byte -> `count` copies of `byte` (count >= 1);  else literal.
fn rleEscapeEnc(d: &[u8], min_run: usize, esc: u8) -> Vec<u8>
{
    let min_run = min_run.max(2);
    let mut out = Vec::new();
    let n = d.len();
    let mut i = 0usize;
    while i < n
    {
        let b = d[i];
        let mut j = i + 1;
        while j < n && d[j] == b && (j - i) < 255
        {
            j += 1;
        }
        let run = j - i;
        if run >= min_run
        {
            out.push(esc);
            out.push(run as u8);
            out.push(b);
            i = j;
        }
        else
        {
            for _ in 0..run
            {
                if b == esc
                {
                    out.push(esc);
                    out.push(0);
                }
                else
                {
                    out.push(b);
                }
            }
            i = j;
        }
    }
    out
}

fn rleEscapeDec(d: &[u8], esc: u8) -> Result<Vec<u8>, String>
{
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < d.len()
    {
        if d[i] == esc
        {
            if i + 1 >= d.len()
            {
                return Err("rle_escape: truncated escape".into());
            }
            let count = d[i + 1];
            if count == 0
            {
                out.push(esc);
                i += 2;
            }
            else
            {
                if i + 2 >= d.len()
                {
                    return Err("rle_escape: truncated run".into());
                }
                out.extend(std::iter::repeat(d[i + 2]).take(count as usize));
                i += 3;
            }
        }
        else
        {
            out.push(d[i]);
            i += 1;
        }
    }
    Ok(out)
}

/// Zero-run RLE:  ESC count -> `count` zeros (count >= 1);  ESC 0 -> literal ESC.
fn zeroRleEnc(d: &[u8], min_run: usize, esc: u8) -> Vec<u8>
{
    let min_run = min_run.max(2);
    let mut out = Vec::new();
    let n = d.len();
    let mut i = 0usize;
    while i < n
    {
        if d[i] == 0
        {
            let mut j = i;
            while j < n && d[j] == 0 && (j - i) < 255
            {
                j += 1;
            }
            let run = j - i;
            if run >= min_run
            {
                out.push(esc);
                out.push(run as u8);
                i = j;
                continue;
            }
        }
        let b = d[i];
        if b == esc
        {
            out.push(esc);
            out.push(0);
        }
        else
        {
            out.push(b);
        }
        i += 1;
    }
    out
}

fn zeroRleDec(d: &[u8], esc: u8) -> Result<Vec<u8>, String>
{
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < d.len()
    {
        if d[i] == esc
        {
            if i + 1 >= d.len()
            {
                return Err("zero_rle: truncated escape".into());
            }
            let count = d[i + 1];
            if count == 0
            {
                out.push(esc);
            }
            else
            {
                out.extend(std::iter::repeat(0u8).take(count as usize));
            }
            i += 2;
        }
        else
        {
            out.push(d[i]);
            i += 1;
        }
    }
    Ok(out)
}

fn packbitsEnc(d: &[u8]) -> Vec<u8>
{
    let mut out = Vec::new();
    let n = d.len();
    let mut i = 0usize;
    while i < n
    {
        let mut run = 1usize;
        while i + run < n && run < 128 && d[i + run] == d[i]
        {
            run += 1;
        }
        if run >= 3
        {
            out.push((257 - run) as u8);
            out.push(d[i]);
            i += run;
            continue;
        }
        let lit_start = i;
        i += 1;
        while i < n && (i - lit_start) < 128
        {
            let mut r = 1usize;
            while i + r < n && r < 128 && d[i + r] == d[i]
            {
                r += 1;
            }
            if r >= 3
            {
                break;
            }
            i += 1;
        }
        out.push((i - lit_start - 1) as u8);
        out.extend_from_slice(&d[lit_start..i]);
    }
    out
}

fn packbitsDec(d: &[u8]) -> Result<Vec<u8>, String>
{
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < d.len()
    {
        let t = d[i] as i32;
        i += 1;
        if t == 128
        {
            continue;
        }
        else if t < 128
        {
            let len = (t + 1) as usize;
            if i + len > d.len()
            {
                return Err("packbits: truncated literal".into());
            }
            out.extend_from_slice(&d[i..i + len]);
            i += len;
        }
        else
        {
            if i >= d.len()
            {
                return Err("packbits: truncated run".into());
            }
            out.extend(std::iter::repeat(d[i]).take((257 - t) as usize));
            i += 1;
        }
    }
    Ok(out)
}

fn mtfEnc(d: &[u8]) -> Vec<u8>
{
    let mut table: [u8; 256] = [0; 256];
    for i in 0..256
    {
        table[i] = i as u8;
    }
    let mut pos: [u8; 256] = table;
    let mut out = Vec::with_capacity(d.len());
    for &b in d
    {
        let idx = pos[b as usize] as usize;
        out.push(idx as u8);
        if idx > 0
        {
            let mut k = idx;
            while k > 0
            {
                let sym = table[k - 1];
                table[k] = sym;
                pos[sym as usize] = k as u8;
                k -= 1;
            }
            table[0] = b;
            pos[b as usize] = 0;
        }
    }
    out
}

fn mtfDec(d: &[u8]) -> Vec<u8>
{
    let mut table: [u8; 256] = [0; 256];
    for i in 0..256
    {
        table[i] = i as u8;
    }
    let mut out = Vec::with_capacity(d.len());
    for &idx in d
    {
        let idx = idx as usize;
        let b = table[idx];
        out.push(b);
        if idx > 0
        {
            let mut k = idx;
            while k > 0
            {
                table[k] = table[k - 1];
                k -= 1;
            }
            table[0] = b;
        }
    }
    out
}

/// Bitplane split. Header: "BP8\0" + u32be original length.
fn bitplaneEnc(d: &[u8]) -> Vec<u8>
{
    let n = d.len();
    let plane_bytes = n.div_ceil(8);
    let mut out = Vec::with_capacity(8 + 8 * plane_bytes);
    out.extend_from_slice(b"BP8\0");
    out.extend_from_slice(&(n as u32).to_be_bytes());
    for bit in 0..8u32
    {
        let mut acc = 0u8;
        let mut cnt = 0u32;
        for &b in d
        {
            acc = (acc << 1) | ((b >> bit) & 1);
            cnt += 1;
            if cnt == 8
            {
                out.push(acc);
                acc = 0;
                cnt = 0;
            }
        }
        if cnt > 0
        {
            out.push(acc << (8 - cnt));
        }
    }
    out
}

fn bitplaneDec(d: &[u8]) -> Result<Vec<u8>, String>
{
    if d.len() < 8 || &d[0..4] != b"BP8\0"
    {
        return Err("bitplane: bad header".into());
    }
    let n = u32::from_be_bytes([d[4], d[5], d[6], d[7]]) as usize;
    let plane_bytes = n.div_ceil(8);
    // saturating: on 32-bit targets 8 * plane_bytes could wrap and pass this check.
    if d.len() < plane_bytes.saturating_mul(8).saturating_add(8)
    {
        return Err("bitplane: truncated planes".into());
    }
    let mut out = vec![0u8; n];
    for bit in 0..8u32
    {
        let base = 8 + bit as usize * plane_bytes;
        for i in 0..n
        {
            let byte = d[base + i / 8];
            let v = (byte >> (7 - (i % 8))) & 1;
            out[i] |= v << bit;
        }
    }
    Ok(out)
}

/// Byte-pair substitution. Header: "BPE1" + rule_count + [sym, a, b] * count.
fn bpeEnc(d: &[u8], max_rules: usize) -> Vec<u8>
{
    let mut stream = d.to_vec();
    let mut used = [false; 256];
    for &b in d
    {
        used[b as usize] = true;
    }
    let available: Vec<u8> = (0..256).filter(|&i| !used[i]).map(|i| i as u8).collect();
    let mut rules: Vec<(u8, u8, u8)> = Vec::new();

    for &sym in available.iter().take(max_rules.min(255))
    {
        if stream.len() < 2
        {
            break;
        }
        let mut counts = vec![0u32; 65536];
        for w in stream.windows(2)
        {
            counts[((w[0] as usize) << 8) | w[1] as usize] += 1;
        }
        let (best_idx, best_cnt) = counts
            .iter()
            .enumerate()
            .max_by_key(|&(_, c)| *c)
            .map(|(i, c)| (i, *c))
            .unwrap_or((0, 0));
        if best_cnt < 4
        {
            break;
        }
        let (a, b) = ((best_idx >> 8) as u8, (best_idx & 0xFF) as u8);
        let mut ns = Vec::with_capacity(stream.len());
        let mut i = 0usize;
        while i < stream.len()
        {
            if i + 1 < stream.len() && stream[i] == a && stream[i + 1] == b
            {
                ns.push(sym);
                i += 2;
            }
            else
            {
                ns.push(stream[i]);
                i += 1;
            }
        }
        if ns.len() >= stream.len()
        {
            break;
        }
        rules.push((sym, a, b));
        stream = ns;
    }

    let mut out = Vec::with_capacity(5 + rules.len() * 3 + stream.len());
    out.extend_from_slice(b"BPE1");
    out.push(rules.len() as u8);
    for (s, a, b) in &rules
    {
        out.extend_from_slice(&[*s, *a, *b]);
    }
    out.extend_from_slice(&stream);
    out
}

fn bpeDec(d: &[u8]) -> Result<Vec<u8>, String>
{
    if d.len() < 5 || &d[0..4] != b"BPE1"
    {
        return Err("bpe: bad header".into());
    }
    let count = d[4] as usize;
    if d.len() < 5 + count * 3
    {
        return Err("bpe: truncated rules".into());
    }
    let rules: Vec<(u8, u8, u8)> = (0..count)
        .map(|i| (d[5 + i * 3], d[6 + i * 3], d[7 + i * 3]))
        .collect();
    let mut stream = d[5 + count * 3..].to_vec();
    for &(sym, a, b) in rules.iter().rev()
    {
        let mut ns = Vec::with_capacity(stream.len() * 2);
        for &c in &stream
        {
            if c == sym
            {
                ns.push(a);
                ns.push(b);
            }
            else
            {
                ns.push(c);
            }
        }
        stream = ns;
    }
    Ok(stream)
}

// ---------------------------------------------------------------------------
// LZ family
// ---------------------------------------------------------------------------

fn varintPut(mut n: usize, out: &mut Vec<u8>)
{
    loop
    {
        let b = (n & 0x7F) as u8;
        n >>= 7;
        if n != 0
        {
            out.push(b | 0x80);
        }
        else
        {
            out.push(b);
            break;
        }
    }
}

fn varintGet(d: &[u8], i: &mut usize) -> Result<usize, String>
{
    let mut n = 0usize;
    let mut shift = 0u32;
    loop
    {
        if *i >= d.len()
        {
            return Err("varint: truncated".into());
        }
        let b = d[*i];
        *i += 1;
        n |= ((b & 0x7F) as usize) << shift;
        if b & 0x80 == 0
        {
            break;
        }
        shift += 7;
        if shift > 63
        {
            return Err("varint: overflow".into());
        }
    }
    Ok(n)
}

#[inline]
fn copyMatch(out: &mut Vec<u8>, dist: usize, len: usize) -> Result<(), String>
{
    if dist == 0 || dist > out.len()
    {
        return Err(format!("lz: bad distance {} (have {})", dist, out.len()));
    }
    let start = out.len() - dist;
    for k in 0..len
    {
        let b = out[start + k];
        out.push(b);
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Tok
{
    Lit(u8),
    Match(u32, u32), // (dist, len)
}

fn lzParse(
    d: &[u8],
    window: usize,
    hash_len: usize,
    min_match: usize,
    max_match: usize,
    max_chain: usize,
) -> Vec<Tok>
{
    let n = d.len();
    let mut mf = MatchFinder::new(d, hash_len);
    let mut toks = Vec::with_capacity(n / 2 + 1);
    let mut i = 0usize;
    while i < n
    {
        let (len, dist) = mf.find(i, window, min_match, max_match, max_chain);
        if len >= min_match
        {
            toks.push(Tok::Match(dist as u32, len as u32));
            for q in i..i + len
            {
                mf.insert(q);
            }
            i += len;
        }
        else
        {
            toks.push(Tok::Lit(d[i]));
            mf.insert(i);
            i += 1;
        }
    }
    toks
}

/// LZSS: u32le original length, then groups of 8 tokens behind a flag byte; match = 12-bit dist, 4-bit len.
fn lzssEnc(d: &[u8], p: &Params) -> Vec<u8>
{
    let min_match = p.min_match.max(2);
    let max_match = p.max_match.min(min_match + 15);
    let window = p.window.min(4095);
    let toks = lzParse(d, window, 3, min_match, max_match, p.max_chain);

    let mut out = Vec::with_capacity(d.len());
    out.extend_from_slice(&(d.len() as u32).to_le_bytes());
    for group in toks.chunks(8)
    {
        let flags_pos = out.len();
        out.push(0);
        let mut flags = 0u8;
        for (bit, t) in group.iter().enumerate()
        {
            match *t
            {
                Tok::Match(dist, len) =>
                {
                    flags |= 1 << bit;
                    let v = ((dist as u16) << 4) | ((len as u16) - min_match as u16);
                    out.extend_from_slice(&v.to_be_bytes());
                }
                Tok::Lit(b) => out.push(b),
            }
        }
        out[flags_pos] = flags;
    }
    out
}

fn lzssDec(d: &[u8], p: &Params) -> Result<Vec<u8>, String>
{
    if d.len() < 4
    {
        return if d.is_empty()
        {
            Ok(Vec::new())
        }
        else
        {
            Err("lzss: truncated header".into())
        };
    }
    let min_match = p.min_match.max(2);
    let n = u32::from_le_bytes([d[0], d[1], d[2], d[3]]) as usize;

    // LZSS expands at most ~8x, so a length header far beyond that is corrupt,
    // not a large file; without this check 0xFFFFFFFF asks the allocator for 4 GB.
    let max_plausible = d.len().saturating_mul(32).saturating_add(1024);
    if n > max_plausible
    {
        return Err(format!(
            "lzss: declared length {} implausible for {} input bytes",
            n,
            d.len()
        ));
    }
    let mut out = Vec::with_capacity(n);
    let mut i = 4usize;
    while out.len() < n
    {
        if i >= d.len()
        {
            return Err("lzss: truncated stream".into());
        }
        let flags = d[i];
        i += 1;
        for bit in 0..8
        {
            if out.len() >= n
            {
                break;
            }
            if flags & (1 << bit) != 0
            {
                if i + 1 >= d.len()
                {
                    return Err("lzss: truncated match".into());
                }
                let v = u16::from_be_bytes([d[i], d[i + 1]]);
                i += 2;
                let dist = (v >> 4) as usize;
                let len = (v & 0xF) as usize + min_match;
                copyMatch(&mut out, dist, len)?;
            }
            else
            {
                if i >= d.len()
                {
                    return Err("lzss: truncated literal".into());
                }
                out.push(d[i]);
                i += 1;
            }
        }
    }
    Ok(out)
}

/// Verbose tokens: [0][byte] | [1][dist:u16be][len:u8]
fn tokensEnc(d: &[u8], p: &Params) -> Vec<u8>
{
    let min_match = p.min_match.max(3);
    let toks = lzParse(
        d,
        p.window.min(65535),
        3,
        min_match,
        p.max_match.min(255),
        p.max_chain,
    );
    let mut out = Vec::with_capacity(d.len());
    for t in &toks
    {
        match *t
        {
            Tok::Lit(b) =>
            {
                out.push(0);
                out.push(b);
            }
            Tok::Match(dist, len) =>
            {
                out.push(1);
                out.extend_from_slice(&(dist as u16).to_be_bytes());
                out.push(len as u8);
            }
        }
    }
    out
}

fn tokensDec(d: &[u8]) -> Result<Vec<u8>, String>
{
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < d.len()
    {
        match d[i]
        {
            0 =>
            {
                if i + 1 >= d.len()
                {
                    return Err("tokens: truncated literal".into());
                }
                out.push(d[i + 1]);
                i += 2;
            }
            1 =>
            {
                if i + 3 >= d.len()
                {
                    return Err("tokens: truncated match".into());
                }
                let dist = u16::from_be_bytes([d[i + 1], d[i + 2]]) as usize;
                let len = d[i + 3] as usize;
                copyMatch(&mut out, dist, len)?;
                i += 4;
            }
            t => return Err(format!("tokens: bad tag {}", t)),
        }
    }
    Ok(out)
}

/// Short-distance tokens: [0][byte] | [2][dist:u8][len:u8] | [1][dist:u16be][len:u8]
fn shortdistEnc(d: &[u8], p: &Params) -> Vec<u8>
{
    let min_match = p.min_match.max(3);
    let toks = lzParse(
        d,
        p.window.min(65535),
        3,
        min_match,
        p.max_match.min(255),
        p.max_chain,
    );
    let mut out = Vec::with_capacity(d.len());
    for t in &toks
    {
        match *t
        {
            Tok::Lit(b) =>
            {
                out.push(0);
                out.push(b);
            }
            Tok::Match(dist, len) if dist <= 255 =>
            {
                out.extend_from_slice(&[2, dist as u8, len as u8]);
            }
            Tok::Match(dist, len) =>
            {
                out.push(1);
                out.extend_from_slice(&(dist as u16).to_be_bytes());
                out.push(len as u8);
            }
        }
    }
    out
}

fn shortdistDec(d: &[u8]) -> Result<Vec<u8>, String>
{
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < d.len()
    {
        match d[i]
        {
            0 =>
            {
                if i + 1 >= d.len()
                {
                    return Err("shortdist: truncated literal".into());
                }
                out.push(d[i + 1]);
                i += 2;
            }
            2 =>
            {
                if i + 2 >= d.len()
                {
                    return Err("shortdist: truncated short match".into());
                }
                copyMatch(&mut out, d[i + 1] as usize, d[i + 2] as usize)?;
                i += 3;
            }
            1 =>
            {
                if i + 3 >= d.len()
                {
                    return Err("shortdist: truncated match".into());
                }
                let dist = u16::from_be_bytes([d[i + 1], d[i + 2]]) as usize;
                copyMatch(&mut out, dist, d[i + 3] as usize)?;
                i += 4;
            }
            t => return Err(format!("shortdist: bad tag {}", t)),
        }
    }
    Ok(out)
}

/// Varint tokens: [0][byte] | [1][varint dist][varint len]
fn varintLzEnc(d: &[u8], p: &Params) -> Vec<u8>
{
    let min_match = p.min_match.max(3);
    let toks = lzParse(
        d,
        p.window.min(65535),
        3,
        min_match,
        p.max_match.min(255),
        p.max_chain,
    );
    let mut out = Vec::with_capacity(d.len());
    for t in &toks
    {
        match *t
        {
            Tok::Lit(b) =>
            {
                out.push(0);
                out.push(b);
            }
            Tok::Match(dist, len) =>
            {
                out.push(1);
                varintPut(dist as usize, &mut out);
                varintPut(len as usize, &mut out);
            }
        }
    }
    out
}

fn varintLzDec(d: &[u8]) -> Result<Vec<u8>, String>
{
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < d.len()
    {
        match d[i]
        {
            0 =>
            {
                if i + 1 >= d.len()
                {
                    return Err("varint_lz: truncated literal".into());
                }
                out.push(d[i + 1]);
                i += 2;
            }
            1 =>
            {
                i += 1;
                let dist = varintGet(d, &mut i)?;
                let len = varintGet(d, &mut i)?;
                copyMatch(&mut out, dist, len)?;
            }
            t => return Err(format!("varint_lz: bad tag {}", t)),
        }
    }
    Ok(out)
}

/// Literal-run tokens: [0][len:u8][bytes...] | [1][dist:u16be][len:u8]
fn litrunEnc(d: &[u8], p: &Params) -> Vec<u8>
{
    let min_match = p.min_match.max(4);
    let toks = lzParse(
        d,
        p.window.min(65535),
        3,
        min_match,
        p.max_match.min(255),
        p.max_chain,
    );
    let mut out: Vec<u8> = Vec::with_capacity(d.len());
    let mut lits: Vec<u8> = Vec::new();

    let flush = |lits: &mut Vec<u8>, out: &mut Vec<u8>| {
        for chunk in lits.chunks(255)
        {
            out.push(0);
            out.push(chunk.len() as u8);
            out.extend_from_slice(chunk);
        }
        lits.clear();
    };

    for t in &toks
    {
        match *t
        {
            Tok::Lit(b) => lits.push(b),
            Tok::Match(dist, len) =>
            {
                flush(&mut lits, &mut out);
                out.push(1);
                out.extend_from_slice(&(dist as u16).to_be_bytes());
                out.push(len as u8);
            }
        }
    }
    flush(&mut lits, &mut out);
    out
}

fn litrunDec(d: &[u8]) -> Result<Vec<u8>, String>
{
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < d.len()
    {
        match d[i]
        {
            0 =>
            {
                if i + 1 >= d.len()
                {
                    return Err("litrun: truncated run header".into());
                }
                let len = d[i + 1] as usize;
                if i + 2 + len > d.len()
                {
                    return Err("litrun: truncated literals".into());
                }
                out.extend_from_slice(&d[i + 2..i + 2 + len]);
                i += 2 + len;
            }
            1 =>
            {
                if i + 3 >= d.len()
                {
                    return Err("litrun: truncated match".into());
                }
                let dist = u16::from_be_bytes([d[i + 1], d[i + 2]]) as usize;
                copyMatch(&mut out, dist, d[i + 3] as usize)?;
                i += 4;
            }
            t => return Err(format!("litrun: bad tag {}", t)),
        }
    }
    Ok(out)
}

/// LZ4-like sequences: [token][ext lit len][literals][offset u16le][ext match len]
fn lz4ishEnc(d: &[u8], p: &Params) -> Vec<u8>
{
    const MIN_MATCH: usize = 4;
    let n = d.len();
    let window = p.window.min(65535);
    let max_match = p.max_match.max(64);
    let mut out = Vec::with_capacity(n / 2 + 16);
    let mut mf = MatchFinder::new(d, 4);
    let mut i = 0usize;
    let mut anchor = 0usize;

    let put_ext = |mut x: usize, o: &mut Vec<u8>| {
        while x >= 255
        {
            o.push(255);
            x -= 255;
        }
        o.push(x as u8);
    };

    while i + MIN_MATCH <= n
    {
        let (len, dist) = mf.find(i, window, MIN_MATCH, max_match, p.max_chain);
        if len >= MIN_MATCH
        {
            let lit_len = i - anchor;
            let ml = len - MIN_MATCH;
            let tok = ((if lit_len >= 15 { 15 } else { lit_len }) << 4)
                | (if ml >= 15 { 15 } else { ml });
            out.push(tok as u8);
            if lit_len >= 15
            {
                put_ext(lit_len - 15, &mut out);
            }
            out.extend_from_slice(&d[anchor..i]);
            out.extend_from_slice(&(dist as u16).to_le_bytes());
            if ml >= 15
            {
                put_ext(ml - 15, &mut out);
            }
            for q in i..i + len
            {
                mf.insert(q);
            }
            i += len;
            anchor = i;
        }
        else
        {
            mf.insert(i);
            i += 1;
        }
    }
    let lit_len = n - anchor;
    let tok = if lit_len >= 15
    {
        15usize << 4
    }
    else
    {
        lit_len << 4
    };
    out.push(tok as u8);
    if lit_len >= 15
    {
        put_ext(lit_len - 15, &mut out);
    }
    out.extend_from_slice(&d[anchor..]);
    out
}

fn lz4ishDec(d: &[u8]) -> Result<Vec<u8>, String>
{
    const MIN_MATCH: usize = 4;
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < d.len()
    {
        let tok = d[i];
        i += 1;
        let mut lit_len = (tok >> 4) as usize;
        if lit_len == 15
        {
            loop
            {
                if i >= d.len()
                {
                    return Err("lz4ish: truncated lit ext".into());
                }
                let b = d[i];
                i += 1;
                lit_len += b as usize;
                if b != 255
                {
                    break;
                }
            }
        }
        if i + lit_len > d.len()
        {
            return Err("lz4ish: truncated literals".into());
        }
        out.extend_from_slice(&d[i..i + lit_len]);
        i += lit_len;
        if i >= d.len()
        {
            break; // final literals-only sequence
        }
        if i + 1 >= d.len()
        {
            return Err("lz4ish: truncated offset".into());
        }
        let dist = u16::from_le_bytes([d[i], d[i + 1]]) as usize;
        i += 2;
        let mut ml = (tok & 0x0F) as usize;
        if ml == 15
        {
            loop
            {
                if i >= d.len()
                {
                    return Err("lz4ish: truncated match ext".into());
                }
                let b = d[i];
                i += 1;
                ml += b as usize;
                if b != 255
                {
                    break;
                }
            }
        }
        copyMatch(&mut out, dist, ml + MIN_MATCH)?;
    }
    Ok(out)
}

/// Printable hex rendering of the verbose token stream: L<hh> / M<dddd><ll>.
fn asciiTokensEnc(d: &[u8], p: &Params) -> Vec<u8>
{
    let min_match = p.min_match.max(4);
    let toks = lzParse(
        d,
        p.window.min(65535),
        3,
        min_match,
        p.max_match.min(255),
        p.max_chain,
    );
    let hx = b"0123456789abcdef";
    let mut out = Vec::with_capacity(d.len() * 2);
    for t in &toks
    {
        match *t
        {
            Tok::Lit(b) =>
            {
                out.push(b'L');
                out.push(hx[(b >> 4) as usize]);
                out.push(hx[(b & 0xF) as usize]);
            }
            Tok::Match(dist, len) =>
            {
                out.push(b'M');
                for s in [12u32, 8, 4, 0]
                {
                    out.push(hx[((dist >> s) & 0xF) as usize]);
                }
                out.push(hx[((len >> 4) & 0xF) as usize]);
                out.push(hx[(len & 0xF) as usize]);
            }
        }
    }
    out
}

fn hexval(b: u8) -> Result<u32, String>
{
    match b
    {
        b'0'..=b'9' => Ok((b - b'0') as u32),
        b'a'..=b'f' => Ok((b - b'a' + 10) as u32),
        _ => Err("ascii_tokens: bad hex".into()),
    }
}

fn asciiTokensDec(d: &[u8]) -> Result<Vec<u8>, String>
{
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < d.len()
    {
        match d[i]
        {
            b'L' =>
            {
                if i + 2 >= d.len()
                {
                    return Err("ascii_tokens: truncated literal".into());
                }
                out.push(((hexval(d[i + 1])? << 4) | hexval(d[i + 2])?) as u8);
                i += 3;
            }
            b'M' =>
            {
                if i + 6 >= d.len()
                {
                    return Err("ascii_tokens: truncated match".into());
                }
                let mut dist = 0u32;
                for k in 1..5
                {
                    dist = (dist << 4) | hexval(d[i + k])?;
                }
                let len = (hexval(d[i + 5])? << 4) | hexval(d[i + 6])?;
                copyMatch(&mut out, dist as usize, len as usize)?;
                i += 7;
            }
            _ => return Err("ascii_tokens: bad tag".into()),
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Heatshrink-compatible bitstream (window=8, lookahead=4)
// ---------------------------------------------------------------------------

struct BitWriter
{
    out: Vec<u8>,
    byte: u8,
    used: u8,
}

impl BitWriter
{
    fn new(capacity: usize) -> Self
    {
        Self {
            out: Vec::with_capacity(capacity),
            byte: 0,
            used: 0,
        }
    }

    fn bits(&mut self, value: usize, count: u8)
    {
        for shift in (0..count).rev()
        {
            self.byte = (self.byte << 1) | ((value >> shift) as u8 & 1);
            self.used += 1;
            if self.used == 8
            {
                self.out.push(self.byte);
                self.byte = 0;
                self.used = 0;
            }
        }
    }

    fn finish(mut self) -> Vec<u8>
    {
        if self.used != 0
        {
            self.out.push(self.byte << (8 - self.used));
        }
        self.out
    }
}

struct BitReader<'a>
{
    data: &'a [u8],
    bit: usize,
}

impl<'a> BitReader<'a>
{
    fn bits(&mut self, count: u8) -> Option<usize>
    {
        if self.bit.checked_add(count as usize)? > self.data.len() * 8
        {
            return None;
        }
        let mut value = 0usize;
        for _ in 0..count
        {
            value =
                (value << 1) | ((self.data[self.bit >> 3] >> (7 - (self.bit & 7))) & 1) as usize;
            self.bit += 1;
        }
        Some(value)
    }
}

/// Heatshrink payload with a small self-describing wrapper:
/// `HSK1`, u32le output length, then the standard MSB-first stream.
fn heatshrinkEnc(d: &[u8], p: &Params) -> Vec<u8>
{
    const WINDOW: usize = 1 << 8;
    const MAX_MATCH: usize = 1 << 4;
    const MIN_MATCH: usize = 3;

    let mut bits = BitWriter::new(d.len());
    let mut finder = MatchFinder::new(d, 3);
    let mut i = 0usize;
    while i < d.len()
    {
        let (len, dist) = finder.find(i, WINDOW, MIN_MATCH, MAX_MATCH, p.max_chain);
        if len >= MIN_MATCH
        {
            bits.bits(0, 1);
            bits.bits(dist - 1, 8);
            bits.bits(len - 1, 4);
            for j in i..i + len
            {
                finder.insert(j);
            }
            i += len;
        }
        else
        {
            bits.bits(1, 1);
            bits.bits(d[i] as usize, 8);
            finder.insert(i);
            i += 1;
        }
    }

    let payload = bits.finish();
    let mut out = Vec::with_capacity(8 + payload.len());
    out.extend_from_slice(b"HSK1");
    out.extend_from_slice(&(d.len() as u32).to_le_bytes());
    out.extend_from_slice(&payload);
    out
}

fn heatshrinkDec(d: &[u8]) -> Result<Vec<u8>, String>
{
    if d.len() < 8 || &d[..4] != b"HSK1"
    {
        return Err("heatshrink: bad header".into());
    }
    let n = u32::from_le_bytes([d[4], d[5], d[6], d[7]]) as usize;
    let max_plausible = d.len().saturating_mul(16).saturating_add(16);
    if n > max_plausible
    {
        return Err(format!("heatshrink: declared length {} implausible", n));
    }

    let mut bits = BitReader {
        data: &d[8..],
        bit: 0,
    };
    let mut out = Vec::with_capacity(n);
    while out.len() < n
    {
        if bits.bits(1).ok_or("heatshrink: truncated tag")? != 0
        {
            let b = bits.bits(8).ok_or("heatshrink: truncated literal")? as u8;
            out.push(b);
        }
        else
        {
            let dist = bits.bits(8).ok_or("heatshrink: truncated distance")? + 1;
            let len = bits.bits(4).ok_or("heatshrink: truncated length")? + 1;
            if dist > out.len() || out.len() + len > n
            {
                return Err("heatshrink: invalid back-reference".into());
            }
            for _ in 0..len
            {
                out.push(out[out.len() - dist]);
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

#[allow(dead_code)] // Kept as a compatibility view; new consumers use registry::ALL.
pub const CODEC_NAMES: &[&str] = &[
    "dpcm",
    "xor_prev",
    "delta16",
    "rle_pair",
    "rle_escape",
    "zero_rle",
    "packbits",
    "mtf",
    "bitplane_split",
    "bpe",
    "lzss",
    "lz77_tokens",
    "lz77_shortdist",
    "lz77_varint",
    "lz77_litrun",
    "lz4ish",
    "lz77_ascii_tokens",
    "heatshrink",
    "rans",
    "dpcm_rans",
    "mtf_rans",
    "bitplane_rans",
];

pub fn encodeOne(name: &str, d: &[u8], p: &Params) -> Result<Vec<u8>, String>
{
    Ok(match name
    {
        "dpcm" => dpcmEnc(d),
        "xor_prev" => xorEnc(d),
        "delta16" => delta16Enc(d),
        "rle_pair" => rlePairEnc(d),
        "rle_escape" => rleEscapeEnc(d, p.min_run, p.esc),
        "zero_rle" => zeroRleEnc(d, p.min_run, p.esc),
        "packbits" => packbitsEnc(d),
        "mtf" => mtfEnc(d),
        "bitplane_split" => bitplaneEnc(d),
        "bpe" => bpeEnc(d, p.bpe_rules),
        "lzss" => lzssEnc(d, p),
        "lz77_tokens" => tokensEnc(d, p),
        "lz77_shortdist" => shortdistEnc(d, p),
        "lz77_varint" => varintLzEnc(d, p),
        "lz77_litrun" => litrunEnc(d, p),
        "lz4ish" => lz4ishEnc(d, p),
        "lz77_ascii_tokens" => asciiTokensEnc(d, p),
        "heatshrink" => heatshrinkEnc(d, p),
        "rans" => ransEncode(d),
        "dpcm_rans" => ransEncode(&dpcmEnc(d)),
        "mtf_rans" => ransEncode(&mtfEnc(d)),
        "bitplane_rans" => ransEncode(&bitplaneEnc(d)),
        other => return Err(format!("unknown codec: {}", other)),
    })
}

pub fn decodeOne(name: &str, d: &[u8], p: &Params) -> Result<Vec<u8>, String>
{
    match name
    {
        "dpcm" => Ok(dpcmDec(d)),
        "xor_prev" => Ok(xorDec(d)),
        "delta16" => Ok(delta16Dec(d)),
        "rle_pair" => rlePairDec(d),
        "rle_escape" => rleEscapeDec(d, p.esc),
        "zero_rle" => zeroRleDec(d, p.esc),
        "packbits" => packbitsDec(d),
        "mtf" => Ok(mtfDec(d)),
        "bitplane_split" => bitplaneDec(d),
        "bpe" => bpeDec(d),
        "lzss" => lzssDec(d, p),
        "lz77_tokens" => tokensDec(d),
        "lz77_shortdist" => shortdistDec(d),
        "lz77_varint" => varintLzDec(d),
        "lz77_litrun" => litrunDec(d),
        "lz4ish" => lz4ishDec(d),
        "lz77_ascii_tokens" => asciiTokensDec(d),
        "heatshrink" => heatshrinkDec(d),
        "rans" => ransDecode(d),
        "dpcm_rans" => Ok(dpcmDec(&ransDecode(d)?)),
        "mtf_rans" => Ok(mtfDec(&ransDecode(d)?)),
        "bitplane_rans" => bitplaneDec(&ransDecode(d)?),
        other => Err(format!("unknown codec: {}", other)),
    }
}

// ---------------------------------------------------------------------------
// Static order-0 rANS
// ---------------------------------------------------------------------------
//
// Format: u32le original_len, 256 u16le frequencies (normalised to sum to
// 1<<12), u32le final state, then the rANS byte stream.

const RANS_BITS: u32 = 12;
const RANS_TOTAL: u32 = 1 << RANS_BITS;
const RANS_L: u32 = 1 << 23; // lower bound of the normalised interval

fn ransNormaliseFreqs(data: &[u8]) -> [u16; 256]
{
    let mut raw = [0u32; 256];
    for &b in data
    {
        raw[b as usize] += 1;
    }
    let total: u32 = data.len() as u32;
    let mut freq = [0u16; 256];
    if total == 0
    {
        return freq;
    }
    // Scale to RANS_TOTAL, guaranteeing every present symbol keeps freq >= 1.
    let mut sum = 0u32;
    for i in 0..256
    {
        if raw[i] == 0
        {
            continue;
        }
        let mut f = ((raw[i] as u64 * RANS_TOTAL as u64) / total as u64) as u32;
        if f == 0
        {
            f = 1;
        }
        freq[i] = f as u16;
        sum += f;
    }
    // Fix rounding drift by adjusting the most frequent symbol.
    let argmax = (0..256).max_by_key(|&i| freq[i]).unwrap();
    let cur = freq[argmax] as i64 + (RANS_TOTAL as i64 - sum as i64);
    freq[argmax] = cur.clamp(1, u16::MAX as i64) as u16;
    freq
}

fn ransCumulative(freq: &[u16; 256]) -> [u32; 257]
{
    let mut cum = [0u32; 257];
    for i in 0..256
    {
        cum[i + 1] = cum[i] + freq[i] as u32;
    }
    cum
}

pub fn ransEncode(data: &[u8]) -> Vec<u8>
{
    if data.is_empty()
    {
        return vec![0, 0, 0, 0];
    }
    let freq = ransNormaliseFreqs(data);
    let cum = ransCumulative(&freq);

    // Encode back-to-front, streaming out low bytes as the state grows.
    let mut stream: Vec<u8> = Vec::with_capacity(data.len() / 2 + 16);
    let mut state: u32 = RANS_L;
    for &b in data.iter().rev()
    {
        let f = freq[b as usize] as u32;
        let c = cum[b as usize];
        // renormalise
        let x_max = ((RANS_L >> RANS_BITS) << 8) * f;
        while state >= x_max
        {
            stream.push((state & 0xff) as u8);
            state >>= 8;
        }
        state = ((state / f) << RANS_BITS) + (state % f) + c;
    }

    let mut out = Vec::with_capacity(stream.len() + 4 + 512);
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    for i in 0..256
    {
        out.extend_from_slice(&freq[i].to_le_bytes());
    }
    out.extend_from_slice(&state.to_le_bytes());
    // stream was built LIFO; decoder reads it in the order we now append.
    stream.reverse();
    out.extend_from_slice(&stream);
    out
}

pub fn ransDecode(d: &[u8]) -> Result<Vec<u8>, String>
{
    if d.len() == 4 && d[..4] == [0, 0, 0, 0]
    {
        return Ok(Vec::new());
    }
    if d.len() < 4 + 512 + 4
    {
        return Err("rans: truncated header".into());
    }
    let n = u32::from_le_bytes([d[0], d[1], d[2], d[3]]) as usize;
    let max_plausible = d.len().saturating_mul(64).saturating_add(1 << 16);
    if n > max_plausible
    {
        return Err(format!("rans: declared length {} implausible", n));
    }
    let mut freq = [0u16; 256];
    for i in 0..256
    {
        let off = 4 + i * 2;
        freq[i] = u16::from_le_bytes([d[off], d[off + 1]]);
    }
    let cum = ransCumulative(&freq);
    // Frequencies must sum to exactly RANS_TOTAL or the slot table indexing overflows.
    if cum[256] != RANS_TOTAL
    {
        return Err(format!(
            "rans: frequencies sum to {}, expected {}",
            cum[256], RANS_TOTAL
        ));
    }
    let mut slot = [0u8; RANS_TOTAL as usize];
    for sym in 0..256
    {
        for s in cum[sym]..cum[sym + 1]
        {
            slot[s as usize] = sym as u8;
        }
    }

    let mut pos = 4 + 512;
    let mut state = u32::from_le_bytes([d[pos], d[pos + 1], d[pos + 2], d[pos + 3]]);
    pos += 4;
    let stream = &d[pos..];
    let mut si = 0usize;

    let mut out = Vec::with_capacity(n);
    for _ in 0..n
    {
        let s = (state & (RANS_TOTAL - 1)) as usize;
        let sym = slot[s];
        out.push(sym);
        let f = freq[sym as usize] as u32;
        let c = cum[sym as usize];
        state = f * (state >> RANS_BITS) + (state & (RANS_TOTAL - 1)) - c;
        // renormalise
        while state < RANS_L
        {
            if si >= stream.len()
            {
                return Err("rans: stream underflow".into());
            }
            state = (state << 8) | stream[si] as u32;
            si += 1;
        }
    }
    Ok(out)
}
