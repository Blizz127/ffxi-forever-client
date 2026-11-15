//! The local bake cache (design section 9). Under the client's cache dir only; never in git, never in an archive.
//! Correctness never depends on it: any mismatch deletes the file and rebakes, and every planned leg is verified
//! against the real collision anyway. All failures are non-fatal.
use super::{BAKE_VERSION, BakeStats, FORMAT, Params, Parity, Span, SpanGrid};
use crate::client::ground::NavView;
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 4] = b"VNAV";
/// Total size cap of the cache directory; oldest-used files are pruned first.
const CAP_BYTES: u64 = 512 * 1024 * 1024;

/// A stable 64-bit word hash (8 bytes at a time; not FNV-1a byte-wise, but fixed forever for a given FORMAT).
fn mix(mut h: u64, bytes: &[u8]) -> u64 {
    let mut it = bytes.chunks_exact(8);
    for c in &mut it {
        h = (h ^ u64::from_le_bytes(c.try_into().unwrap())).wrapping_mul(0x0000_0100_0000_01b3);
        h ^= h >> 29;
    }
    for &b in it.remainder() {
        h = (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3);
    }
    h ^ (bytes.len() as u64).rotate_left(17)
}

/// 128-bit key: format, bake version, every parameter, and the content of the collision arrays the bake reads.
pub fn key_of(v: &NavView, p: &Params) -> u128 {
    let mut head = Vec::new();
    head.extend(FORMAT.to_le_bytes());
    head.extend(BAKE_VERSION.to_le_bytes());
    for w in p.words() {
        head.extend(w.to_bits().to_le_bytes());
    }
    let mut halves = [0u64; 2];
    for (k, seed) in [0xcbf2_9ce4_8422_2325u64, 0x9e37_79b9_7f4a_7c15].into_iter().enumerate() {
        let mut h = mix(seed, &head);
        h = mix(h, bytemuck::cast_slice(&v.tris.iter().flatten().map(|p| p.to_array()).collect::<Vec<_>>()));
        h = mix(h, bytemuck::cast_slice(&v.normals.iter().map(|p| p.to_array()).collect::<Vec<_>>()));
        h = mix(h, v.kinds);
        h = mix(h, bytemuck::cast_slice(v.doors));
        h = mix(h, &v.degenerate.iter().map(|&d| d as u8).collect::<Vec<_>>());
        halves[k] = h;
    }
    ((halves[0] as u128) << 64) | halves[1] as u128
}

/// Tests point the cache at a temp dir through this (a process-wide override, set under the tests' own lock), so no
/// test ever writes to the user's cache and nothing mutates the process environment.
#[cfg(test)]
pub(crate) static ROOT_OVERRIDE: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

pub fn root() -> PathBuf {
    #[cfg(test)]
    if let Some(p) = ROOT_OVERRIDE.lock().ok().and_then(|g| g.clone()) {
        return p.join(format!("v{FORMAT}"));
    }
    match std::env::var_os("NAVGRID_CACHE") {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => crate::config::navgrid_dir(),
    }
    .join(format!("v{FORMAT}"))
}
fn file_for(root: &Path, key: u128) -> PathBuf {
    root.join(format!("{key:032x}.bin"))
}

fn put_stats(b: &mut Vec<u8>, s: &BakeStats) {
    for v in [
        s.tris as u64,
        s.floor_tris as u64,
        s.wall_tris as u64,
        s.ceiling_tris as u64,
        s.door_tris as u64,
        s.columns,
        s.cands as u64,
        s.spans_pre as u64,
        s.spans as u64,
        s.dropped_wall as u64,
        s.dropped_ceiling as u64,
        s.door_spans as u64,
        s.links,
        s.multi_candidate,
        s.max_layers as u64,
        s.bake_ms,
    ] {
        b.extend(v.to_le_bytes());
    }
    for a in parity_arrays(&s.parity) {
        for v in a {
            b.extend(v.to_le_bytes());
        }
    }
}
const STAT_WORDS: usize = 16 + 16;
fn parity_arrays(p: &Parity) -> [[u64; 2]; 8] {
    [
        p.link_agree,
        p.link_refused,
        p.no_link_probe_ok,
        p.no_link_probe_no,
        p.walk_agree,
        p.walk_refused,
        p.no_link_walk_ok,
        p.no_link_walk_no,
    ]
}

fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h = (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

pub fn encode(key: u128, g: &SpanGrid) -> Vec<u8> {
    let mut b: Vec<u8> = Vec::with_capacity(g.spans.len() * 8 + g.col_start.len() + 512);
    b.extend(MAGIC);
    b.extend(FORMAT.to_le_bytes());
    b.extend(key.to_le_bytes());
    for w in g.params.words() {
        b.extend(w.to_bits().to_le_bytes());
    }
    for o in g.origin {
        b.extend(o.to_bits().to_le_bytes());
    }
    for c in g.cols {
        b.extend(c.to_le_bytes());
    }
    b.extend((g.spans.len() as u32).to_le_bytes());
    b.extend((g.doors.len() as u32).to_le_bytes());
    put_stats(&mut b, &g.stats);
    // Per-column counts (a span count never exceeds 255), then the spans and the door ids.
    b.extend(g.col_start.windows(2).map(|w| (w[1] - w[0]) as u8));
    for s in &g.spans {
        b.extend(s.y.to_bits().to_le_bytes());
        b.push(s.moves);
        b.push(s.flags);
        b.extend(s.door.to_le_bytes());
    }
    for d in &g.doors {
        b.extend(d.to_le_bytes());
    }
    let sum = fnv64(&b);
    b.extend(sum.to_le_bytes());
    b
}

pub fn decode(bytes: &[u8], key: u128, p: &Params) -> Option<SpanGrid> {
    if bytes.len() < 8 + 4 + 16 {
        return None;
    }
    let (body, sum) = bytes.split_at(bytes.len() - 8);
    if fnv64(body) != u64::from_le_bytes(sum.try_into().ok()?) {
        return None;
    }
    let mut at = 0usize;
    let mut take = |n: usize| -> Option<&[u8]> {
        let s = body.get(at..at + n)?;
        at += n;
        Some(s)
    };
    if take(4)? != MAGIC {
        return None;
    }
    let u32le = |s: &[u8]| u32::from_le_bytes(s.try_into().unwrap());
    let u64le = |s: &[u8]| u64::from_le_bytes(s.try_into().unwrap());
    if u32le(take(4)?) != FORMAT || u128::from_le_bytes(take(16)?.try_into().ok()?) != key {
        return None;
    }
    let mut words = [0f32; 8];
    for w in &mut words {
        *w = f32::from_bits(u32le(take(4)?));
    }
    if words != p.words() {
        return None;
    }
    let origin = [f32::from_bits(u32le(take(4)?)), f32::from_bits(u32le(take(4)?))];
    let cols = [u32le(take(4)?), u32le(take(4)?)];
    let (n_spans, n_doors) = (u32le(take(4)?) as usize, u32le(take(4)?) as usize);
    let ncols = (cols[0] as u64).checked_mul(cols[1] as u64)? as usize;
    if ncols > 100_000_000 || n_spans > 100_000_000 || n_doors > u16::MAX as usize {
        return None;
    }
    let mut sw = [0u64; STAT_WORDS];
    for w in &mut sw {
        *w = u64le(take(8)?);
    }
    let counts = take(ncols)?;
    let mut col_start = Vec::with_capacity(ncols + 1);
    col_start.push(0u32);
    let mut acc = 0u32;
    for &c in counts {
        acc = acc.checked_add(c as u32)?;
        col_start.push(acc);
    }
    if acc as usize != n_spans {
        return None;
    }
    let raw = take(n_spans.checked_mul(8)?)?;
    let spans: Vec<Span> = raw
        .chunks_exact(8)
        .map(|c| Span {
            y: f32::from_bits(u32le(&c[0..4])),
            moves: c[4],
            flags: c[5],
            door: u16::from_le_bytes([c[6], c[7]]),
        })
        .collect();
    if spans.iter().any(|s| !s.y.is_finite() || s.door as usize > n_doors) {
        return None;
    }
    let doors: Vec<u32> = take(n_doors * 4)?.chunks_exact(4).map(u32le).collect();
    if at != body.len() {
        return None;
    }
    let stats = BakeStats {
        tris: sw[0] as usize,
        floor_tris: sw[1] as usize,
        wall_tris: sw[2] as usize,
        ceiling_tris: sw[3] as usize,
        door_tris: sw[4] as usize,
        columns: sw[5],
        cands: sw[6] as usize,
        spans_pre: sw[7] as usize,
        spans: sw[8] as usize,
        dropped_wall: sw[9] as usize,
        dropped_ceiling: sw[10] as usize,
        door_spans: sw[11] as usize,
        links: sw[12],
        multi_candidate: sw[13],
        max_layers: sw[14] as usize,
        bake_ms: sw[15],
        phase_ms: [0; 4],
        parity: Parity {
            link_agree: [sw[16], sw[17]],
            link_refused: [sw[18], sw[19]],
            no_link_probe_ok: [sw[20], sw[21]],
            no_link_probe_no: [sw[22], sw[23]],
            walk_agree: [sw[24], sw[25]],
            walk_refused: [sw[26], sw[27]],
            no_link_walk_ok: [sw[28], sw[29]],
            no_link_walk_no: [sw[30], sw[31]],
            samples: Vec::new(),
        },
    };
    Some(SpanGrid::new(*p, origin, cols, col_start, spans, doors, stats))
}

pub fn load_in(root: &Path, key: u128, p: &Params) -> Option<SpanGrid> {
    let path = file_for(root, key);
    let bytes = std::fs::read(&path).ok()?;
    match decode(&bytes, key, p) {
        Some(g) => {
            // Mark it recently used for the prune.
            if let Ok(f) = std::fs::File::options().write(true).open(&path) {
                let _ = f.set_modified(std::time::SystemTime::now());
            }
            Some(g)
        }
        None => {
            let _ = std::fs::remove_file(&path);
            None
        }
    }
}
pub fn load(key: u128, p: &Params) -> Option<SpanGrid> {
    load_in(&root(), key, p)
}

/// Temp file in the same directory, then rename; the temp file is removed on any error.
pub fn store_in(root: &Path, key: u128, g: &SpanGrid) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(root)?;
    let dest = file_for(root, key);
    let tmp = root.join(format!(
        "{key:032x}.tmp-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos())
    ));
    let res = std::fs::write(&tmp, encode(key, g)).and_then(|_| std::fs::rename(&tmp, &dest));
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    res.map(|_| dest)
}
pub fn store(key: u128, g: &SpanGrid) {
    let root = root();
    match store_in(&root, key, g) {
        Ok(_) => prune(&root, CAP_BYTES),
        Err(e) => log::info!("navgrid: cache not written ({e})"),
    }
}

/// Deletes stray temp files, then the least recently used files until the directory is within `cap`.
pub fn prune(root: &Path, cap: u64) {
    let Ok(rd) = std::fs::read_dir(root) else { return };
    let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = Vec::new();
    for e in rd.flatten() {
        let path = e.path();
        let Ok(m) = e.metadata() else { continue };
        if path.extension().is_some_and(|x| x != "bin") {
            // A leftover temp file of an interrupted write (older than a minute: not one in progress).
            let old = m.modified().ok().and_then(|t| t.elapsed().ok()).is_some_and(|d| d.as_secs() > 60);
            if old {
                let _ = std::fs::remove_file(&path);
            }
            continue;
        }
        files.push((m.modified().unwrap_or(std::time::UNIX_EPOCH), m.len(), path));
    }
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    let mut used = 0u64;
    for (_, len, path) in files {
        used += len;
        if used > cap {
            let _ = std::fs::remove_file(path);
        }
    }
}
