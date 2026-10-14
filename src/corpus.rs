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

//! Corpus mode: characterise every file under a directory so the outliers rise to the top.

use crate::emap;
use crate::score::{achievableEntropy, Metrics};
use rayon::prelude::*;
use std::path::{Path, PathBuf};

pub struct CorpusEntry
{
    pub path: PathBuf,
    pub size: usize,
    pub entropy: f64,
    pub format: String,
    /// Already-compressed container: a codec sweep over it will find almost nothing.
    pub precompressed: bool,
    pub dominant: emap::Band,
    /// Fraction of the file whose window entropy is >= 7.9 (random-like).
    pub random_frac: f64,
    pub error: Option<String>,
}

fn walk(root: &Path, max_files: usize) -> Vec<PathBuf>
{
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    // Build trees are full of hardlinks (cargo's `entropy` and `entropy-<hash>`); count each inode once.
    #[cfg(unix)]
    let mut seen_inodes: std::collections::HashSet<(u64, u64)> = std::collections::HashSet::new();

    while let Some(dir) = stack.pop()
    {
        let Ok(rd) = std::fs::read_dir(&dir)
        else
        {
            continue;
        };
        for entry in rd.flatten()
        {
            // file_type() does not follow symlinks: a symlink to an ancestor would loop the walker
            // until max_files, and a fifo would block the read forever.
            let Ok(ft) = entry.file_type()
            else
            {
                continue;
            };
            if ft.is_symlink()
            {
                continue;
            }
            let p = entry.path();
            if ft.is_dir()
            {
                stack.push(p);
            }
            else if ft.is_file()
            {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    if let Ok(md) = entry.metadata()
                    {
                        if md.nlink() > 1 && !seen_inodes.insert((md.dev(), md.ino()))
                        {
                            continue;
                        }
                    }
                }
                out.push(p);
                if out.len() >= max_files
                {
                    return out;
                }
            }
        }
    }
    out
}

pub fn analyse(
    root: &Path,
    window: usize,
    max_files: usize,
    threads: Option<usize>,
) -> Vec<CorpusEntry>
{
    let files = walk(root, max_files);
    if let Some(n) = threads
    {
        let _ = rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .build_global();
    }

    files
        .par_iter()
        .map(|path| {
            let data = match std::fs::read(path)
            {
                Ok(d) => d,
                Err(e) =>
                {
                    return CorpusEntry {
                        path: path.clone(),
                        size: 0,
                        entropy: 0.0,
                        format: "?".into(),
                        precompressed: false,
                        dominant: emap::Band::Padding,
                        random_frac: 0.0,
                        error: Some(e.to_string()),
                    }
                }
            };
            if data.is_empty()
            {
                return CorpusEntry {
                    path: path.clone(),
                    size: 0,
                    entropy: 0.0,
                    format: "empty".into(),
                    precompressed: false,
                    dominant: emap::Band::Padding,
                    random_frac: 0.0,
                    error: None,
                };
            }

            let m = Metrics::of(&data);
            let map = emap::compute(&data, window, window);
            let thresh = achievableEntropy(window) * 0.98;
            let random_blocks = map.blocks.iter().filter(|b| b.entropy >= thresh).count();
            let random_frac = if map.blocks.is_empty()
            {
                0.0
            }
            else
            {
                random_blocks as f64 / map.blocks.len() as f64
            };

            let mut span = [0usize; 5];
            for r in &map.regions
            {
                let idx = match r.band
                {
                    emap::Band::Padding => 0,
                    emap::Band::Structured => 1,
                    emap::Band::Code => 2,
                    emap::Band::Compressed => 3,
                    emap::Band::Random => 4,
                };
                span[idx] += r.end - r.start;
            }
            let dominant = match span
                .iter()
                .enumerate()
                .max_by_key(|(_, &v)| v)
                .map(|(i, _)| i)
            {
                Some(0) => emap::Band::Padding,
                Some(1) => emap::Band::Structured,
                Some(2) => emap::Band::Code,
                Some(3) => emap::Band::Compressed,
                _ => emap::Band::Random,
            };

            let ident = crate::format::identify(&data);
            CorpusEntry {
                path: path.clone(),
                size: data.len(),
                entropy: m.entropy,
                precompressed: ident.precompressed,
                format: ident.short,
                dominant,
                random_frac,
                error: None,
            }
        })
        .collect()
}
