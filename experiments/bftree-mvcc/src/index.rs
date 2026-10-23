use super::adapter::{Batch, Error, Result, MAX_KEY, MAX_SCAN_RECORDS, MAX_SCAN_ROWS, MAX_VALUE};
use bf_tree::{BfTree, Config, LeafInsertResult, LeafReadResult, ScanReturnField};
use hawdb_storage::cow::{CowPageWeight, CowSegmentedMap};
use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub type Rows = Vec<(Vec<u8>, Vec<u8>)>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Layout {
    Versioned,
    Hybrid,
    Cow,
}
impl Layout {
    pub fn id(self) -> u64 {
        match self {
            Self::Versioned => 1,
            Self::Hybrid => 2,
            Self::Cow => 3,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Versioned => "versioned",
            Self::Hybrid => "hybrid",
            Self::Cow => "cow",
        }
    }
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "versioned" => Ok(Self::Versioned),
            "hybrid" => Ok(Self::Hybrid),
            "cow" => Ok(Self::Cow),
            _ => Err(Error::Invalid("unknown layout")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Key(Vec<u8>);
impl CowPageWeight for Key {
    fn cow_page_bytes(&self) -> usize {
        self.0.len()
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Value(Arc<Vec<u8>>);
impl CowPageWeight for Value {
    fn cow_page_bytes(&self) -> usize {
        self.0.len()
    }
}
type CowMap = CowSegmentedMap<Key, Value>;

pub enum Index {
    Bf { tree: Arc<BfTree>, layout: Layout },
    Cow(Mutex<CowMap>),
}
pub enum View {
    Bf {
        tree: Arc<BfTree>,
        layout: Layout,
        epoch: u64,
    },
    Cow(CowMap),
}
pub struct Previous<'a> {
    pub epoch: u64,
    pub value: &'a Option<Vec<u8>>,
}
pub enum Prepared {
    Bf,
    Cow(CowMap),
}

struct EncodedKey {
    bytes: [u8; MAX_KEY * 2 + 10],
    len: usize,
}
impl std::ops::Deref for EncodedKey {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}
fn prefix(key: &[u8], namespace: u8) -> EncodedKey {
    let mut encoded = EncodedKey {
        bytes: [0; MAX_KEY * 2 + 10],
        len: key.len() * 2 + 2,
    };
    encoded.bytes[0] = namespace;
    for (i, &b) in key.iter().enumerate() {
        encoded.bytes[1 + i * 2] = 1;
        encoded.bytes[2 + i * 2] = b;
    }
    encoded
}
fn version_key(key: &[u8], epoch: u64) -> EncodedKey {
    let mut encoded = prefix(key, 1);
    encoded.bytes[encoded.len..encoded.len + 8].copy_from_slice(&(!epoch).to_be_bytes());
    encoded.len += 8;
    encoded
}
fn decode_key(bytes: &[u8], versioned: bool) -> Result<(Vec<u8>, u64)> {
    let mut key = Vec::new();
    let mut offset = 1;
    loop {
        match bytes.get(offset) {
            Some(0) => {
                offset += 1;
                break;
            }
            Some(1) => {
                key.push(*bytes.get(offset + 1).ok_or(Error::Invalid("short key"))?);
                offset += 2;
            }
            _ => return Err(Error::Invalid("invalid key encoding")),
        }
    }
    let epoch = if versioned {
        !u64::from_be_bytes(
            bytes
                .get(offset..)
                .ok_or(Error::Invalid("short version"))?
                .try_into()
                .map_err(|_| Error::Invalid("invalid version"))?,
        )
    } else {
        if bytes.len() != offset {
            return Err(Error::Invalid("invalid head key"));
        }
        0
    };
    Ok((key, epoch))
}
fn encode_value<'a>(
    value: &Option<Vec<u8>>,
    epoch: Option<u64>,
    out: &'a mut [u8; MAX_VALUE + 9],
) -> &'a [u8] {
    let offset = if let Some(epoch) = epoch {
        out[..8].copy_from_slice(&epoch.to_be_bytes());
        8
    } else {
        0
    };
    match value {
        None => {
            out[offset] = 0;
            &out[..offset + 1]
        }
        Some(value) => {
            out[offset] = 1;
            out[offset + 1..offset + 1 + value.len()].copy_from_slice(value);
            &out[..offset + 1 + value.len()]
        }
    }
}
fn decode_value(bytes: &[u8]) -> Result<Option<Vec<u8>>> {
    match bytes.first() {
        Some(0) if bytes.len() == 1 => Ok(None),
        Some(1) => Ok(Some(bytes[1..].to_vec())),
        _ => Err(Error::Invalid("invalid value tag")),
    }
}
fn decode_owned_value(mut bytes: Vec<u8>, offset: usize) -> Result<Option<Vec<u8>>> {
    match bytes.get(offset) {
        Some(0) if bytes.len() == offset + 1 => Ok(None),
        Some(1) => {
            let len = bytes.len() - offset - 1;
            bytes.copy_within(offset + 1.., 0);
            bytes.truncate(len);
            Ok(Some(bytes))
        }
        _ => Err(Error::Invalid("invalid value tag")),
    }
}
fn insert(tree: &BfTree, key: &[u8], value: &[u8]) -> Result<()> {
    match tree.insert(key, value) {
        LeafInsertResult::Success => Ok(()),
        LeafInsertResult::InvalidKV(reason) => Err(Error::Index(reason)),
    }
}
fn history_get(tree: &BfTree, key: &[u8], epoch: u64) -> Result<Option<Vec<u8>>> {
    let seek = version_key(key, epoch);
    let mut buffer = [0; MAX_KEY * 2 + MAX_VALUE + 64];
    let mut scan = tree
        .scan_with_count(&seek, 1, ScanReturnField::KeyAndValue)
        .map_err(|e| Error::Index(format!("{e:?}")))?;
    let Some((k, v)) = scan.next(&mut buffer) else {
        return Ok(None);
    };
    if buffer.first() != Some(&1) {
        return Ok(None);
    }
    // The exact encoded prefix identifies this logical key without allocating
    // and decoding another key on every point read.
    if k != seek.len() || buffer[..k - 8] != seek[..seek.len() - 8] {
        return Ok(None);
    }
    let version = !u64::from_be_bytes(buffer[k - 8..k].try_into().expect("epoch bytes"));
    if version > epoch {
        return Ok(None);
    }
    decode_value(&buffer[k..k + v])
}

impl Index {
    pub fn new(layout: Layout) -> Result<Self> {
        if layout == Layout::Cow {
            return Ok(Self::Cow(Mutex::new(CowMap::default())));
        }
        let mut config = Config::default();
        config
            // bf-tree 0.5.6 can scan-promote a clean record-cache mini-page
            // without loading its base buffer. Use whole-page read caching
            // until that dependency path is fixed; keep both layouts identical.
            .read_record_cache(false)
            .cb_min_record_size(4)
            .cb_max_record_size(1400)
            .cb_max_key_len(256)
            .cb_size_byte(4 * 1024 * 1024);
        Ok(Self::Bf {
            tree: Arc::new(
                BfTree::with_config(config, None).map_err(|e| Error::Index(format!("{e:?}")))?,
            ),
            layout,
        })
    }
    pub fn capture(&self, published: &AtomicU64) -> Result<(u64, View)> {
        Ok(match self {
            Self::Bf { tree, layout } => {
                let epoch = published.load(Ordering::Acquire);
                (
                    epoch,
                    View::Bf {
                        tree: Arc::clone(tree),
                        layout: *layout,
                        epoch,
                    },
                )
            }
            Self::Cow(state) => {
                let state = state.lock().map_err(|_| Error::NeedsRecovery)?;
                (published.load(Ordering::Acquire), View::Cow(state.clone()))
            }
        })
    }
    pub fn prepare<'a>(
        &self,
        batch: &Batch,
        mut previous: impl FnMut(&[u8]) -> Result<Option<Previous<'a>>>,
        mut installed: impl FnMut(usize) -> Result<()>,
    ) -> Result<Prepared> {
        let mut cow = match self {
            Self::Cow(state) => Some(state.lock().map_err(|_| Error::NeedsRecovery)?.clone()),
            _ => None,
        };
        let mut encoded_value = [0; MAX_VALUE + 9];
        for (i, change) in batch.writes.iter().enumerate() {
            match self {
                Self::Bf {
                    tree,
                    layout: Layout::Versioned,
                } => insert(
                    tree,
                    &version_key(&change.key, batch.epoch),
                    encode_value(&change.value, None, &mut encoded_value),
                )?,
                Self::Bf { tree, .. } => {
                    let head = prefix(&change.key, 2);
                    // The coordinator already retains exact immutable mutation
                    // receipts for retries. Borrow the previous value rather than
                    // reading and cache-promoting the head again on every update.
                    if let Some(previous) = previous(&change.key)? {
                        insert(
                            tree,
                            &version_key(&change.key, previous.epoch),
                            encode_value(previous.value, None, &mut encoded_value),
                        )?;
                    }
                    insert(
                        tree,
                        &head,
                        encode_value(&change.value, Some(batch.epoch), &mut encoded_value),
                    )?;
                }
                Self::Cow(_) => {
                    let state = cow.as_mut().expect("COW candidate");
                    match &change.value {
                        Some(v) => {
                            state.insert(Key(change.key.clone()), Value(Arc::new(v.clone())));
                        }
                        None => {
                            state.remove(&Key(change.key.clone()));
                        }
                    }
                }
            }
            installed(i + 1)?;
        }
        Ok(cow.map_or(Prepared::Bf, Prepared::Cow))
    }
    pub fn publish(&self, prepared: Prepared, epoch: u64, published: &AtomicU64) -> Result<()> {
        match (self, prepared) {
            (Self::Cow(state), Prepared::Cow(candidate)) => {
                let mut state = state.lock().map_err(|_| Error::NeedsRecovery)?;
                *state = candidate;
                published.store(epoch, Ordering::Release);
            }
            (Self::Bf { .. }, Prepared::Bf) => published.store(epoch, Ordering::Release),
            _ => return Err(Error::Invalid("layout mismatch")),
        }
        Ok(())
    }
}

impl View {
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        match self {
            Self::Cow(state) => Ok(state.get(&Key(key.to_vec())).map(|v| v.0.as_ref().clone())),
            Self::Bf {
                tree,
                layout,
                epoch,
            } => {
                if *layout == Layout::Hybrid {
                    let mut head = [0; MAX_VALUE + 32];
                    match tree.read(&prefix(key, 2), &mut head) {
                        LeafReadResult::Found(n) => {
                            let head = &head[..n as usize];
                            let version = u64::from_be_bytes(
                                head.get(..8)
                                    .ok_or(Error::Invalid("short head"))?
                                    .try_into()
                                    .map_err(|_| Error::Invalid("short head"))?,
                            );
                            if version <= *epoch {
                                return decode_value(&head[8..]);
                            }
                        }
                        LeafReadResult::NotFound => return Ok(None),
                        other => {
                            return Err(Error::Index(format!("Bf-Tree read failed: {other:?}")))
                        }
                    }
                }
                history_get(tree, key, *epoch)
            }
        }
    }
    pub fn scan(&self, start: &[u8], end: &[u8]) -> Result<Rows> {
        if start >= end {
            return Err(Error::Invalid("scan requires start < end"));
        }
        if let Self::Cow(state) = self {
            let mut rows = Vec::new();
            let lower = Key(start.to_vec());
            if let Some(v) = state.get(&lower) {
                rows.push((lower.0.clone(), v.0.as_ref().clone()));
            }
            for (k, v) in state.iter_after(Some(lower)) {
                if k.0.as_slice() >= end {
                    break;
                }
                rows.push((k.0.clone(), v.0.as_ref().clone()));
                if rows.len() > MAX_SCAN_ROWS {
                    return Err(Error::Budget("scan rows"));
                }
            }
            return Ok(rows);
        }
        let Self::Bf {
            tree,
            layout,
            epoch,
        } = self
        else {
            unreachable!()
        };
        let hybrid = *layout == Layout::Hybrid;
        let namespace = if hybrid { 2 } else { 1 };
        let mut resume = prefix(start, namespace).to_vec();
        let upper = prefix(end, namespace);
        let mut result: Rows = Vec::new();
        let mut seen = None;
        let mut work = 0;
        let mut skip_resume = false;
        loop {
            // Own a small batch, then drop the native iterator before any
            // history lookup. Its leaf guard must not cross callbacks/yields.
            let mut batch = Vec::new();
            {
                let mut scan = tree
                    .scan_with_end_key(&resume, &upper, ScanReturnField::KeyAndValue)
                    .map_err(|e| Error::Index(format!("{e:?}")))?;
                let mut buffer = [0; MAX_KEY * 2 + MAX_VALUE + 64];
                while let Some((k, v)) = scan.next(&mut buffer) {
                    if skip_resume && buffer[..k] == resume {
                        continue;
                    }
                    batch.push((buffer[..k].to_vec(), buffer[k..k + v].to_vec()));
                    if batch.len() == 32 {
                        break;
                    }
                }
            }
            if batch.is_empty() {
                break;
            }
            skip_resume = true;
            for (physical, value) in batch {
                resume = physical.clone();
                work += 1;
                if work > MAX_SCAN_RECORDS {
                    return Err(Error::Budget("scan physical records"));
                }
                let (key, version) = decode_key(&physical, !hybrid)?;
                if key.as_slice() >= end {
                    return Ok(result);
                }
                if !hybrid && (version > *epoch || seen.as_ref() == Some(&key)) {
                    continue;
                }
                let visible = if hybrid {
                    let version = u64::from_be_bytes(
                        value
                            .get(..8)
                            .ok_or(Error::Invalid("short head"))?
                            .try_into()
                            .map_err(|_| Error::Invalid("short head"))?,
                    );
                    if version <= *epoch {
                        decode_owned_value(value, 8)?
                    } else {
                        history_get(tree, &key, *epoch)?
                    }
                } else {
                    seen = Some(key.clone());
                    decode_owned_value(value, 0)?
                };
                if let Some(v) = visible {
                    if result.last().is_some_and(|(previous, _)| *previous >= key) {
                        return Err(Error::Invalid("non-monotonic scan"));
                    }
                    result.push((key, v));
                }
                if result.len() > MAX_SCAN_ROWS {
                    return Err(Error::Budget("scan rows"));
                }
            }
        }
        Ok(result)
    }
}
