//! Night light (#36): read and switch it for the `display` hook.
//!
//! Windows keeps the on/off state in an undocumented CloudStore registry value
//! (`HKCU\…\CloudStore\Store\DefaultAccount\Current\default$…bluelightreductionstate\…`,
//! `Data`), a Bond CompactBinary v1 blob. Explorer applies a write live. Format:
//! <https://github.com/kvnxiao/win-nightlight-cli/blob/main/docs/nightlight-registry-format.md> (MIT).
//!
//! Envelope: `{ 0: struct (metadata), 1: struct { 0: u64 Unix seconds, 1: struct { 1: list<int8> inner } } }`.
//! Inner state: field 0 (int32) is present only while night light is on, 10 is
//! the cause (1 = manual), 20 the FILETIME of the last change.
//!
//! A careless writer corrupts this value (win-nightlight-cli's predecessor
//! did), so the codec is generic: every field is parsed and kept, and a blob is
//! written only when re-encoding what was read reproduces it byte for byte.
//! Otherwise the read says `supported: false`. The `Cloud` records are never touched.

use serde_json::{json, Value};

/// Bond CompactBinary v1, generic: parse any value, re-encode it unchanged.
mod bond {
    pub const STOP: u8 = 0;
    pub const STOP_BASE: u8 = 1;
    pub const BOOL: u8 = 2;
    pub const UINT8: u8 = 3;
    pub const UINT16: u8 = 4;
    pub const UINT32: u8 = 5;
    pub const UINT64: u8 = 6;
    pub const FLOAT: u8 = 7;
    pub const DOUBLE: u8 = 8;
    pub const STRING: u8 = 9;
    pub const STRUCT: u8 = 10;
    pub const LIST: u8 = 11;
    pub const SET: u8 = 12;
    pub const MAP: u8 = 13;
    pub const INT8: u8 = 14;
    pub const INT16: u8 = 15;
    pub const INT32: u8 = 16;
    pub const INT64: u8 = 17;
    pub const WSTRING: u8 = 18;

    /// `CB`, version 1.
    pub const MAGIC: [u8; 4] = [0x43, 0x42, 0x01, 0x00];
    /// Deeper nesting than this is not a night light blob.
    const MAX_DEPTH: usize = 16;

    #[derive(Debug, Clone, PartialEq)]
    pub enum Value {
        /// bool, uint8, int8: one raw byte.
        Byte(u8),
        /// uint16/32/64 as is, int16/32/64 zigzagged: the varint's value.
        Varint(u64),
        /// float, double.
        Fixed(Vec<u8>),
        /// string (UTF-8 bytes) or wstring (UTF-16LE bytes).
        Text(Vec<u8>),
        Struct(Vec<Item>),
        /// list or set; `elem` is the raw element-type byte.
        List { elem: u8, items: Vec<Value> },
        Map { key: u8, value: u8, pairs: Vec<(Value, Value)> },
    }

    #[derive(Debug, Clone, PartialEq)]
    pub enum Item {
        Field { id: u16, ty: u8, value: Value },
        /// `BT_STOP_BASE`: the end of base-class fields.
        Base,
    }

    pub fn zigzag(v: i64) -> u64 {
        ((v << 1) ^ (v >> 63)) as u64
    }

    #[cfg(test)]
    pub fn unzigzag(v: u64) -> i64 {
        ((v >> 1) as i64) ^ -((v & 1) as i64)
    }

    struct Reader<'a> {
        data: &'a [u8],
        pos: usize,
    }

    impl Reader<'_> {
        fn byte(&mut self) -> Result<u8, String> {
            let b = *self.data.get(self.pos).ok_or("bond: unexpected end")?;
            self.pos += 1;
            Ok(b)
        }

        fn bytes(&mut self, n: usize) -> Result<Vec<u8>, String> {
            if n > self.data.len() - self.pos {
                return Err("bond: unexpected end".into());
            }
            let out = self.data[self.pos..self.pos + n].to_vec();
            self.pos += n;
            Ok(out)
        }

        fn varint(&mut self) -> Result<u64, String> {
            let mut out = 0u64;
            for i in 0..10 {
                let b = self.byte()?;
                out |= u64::from(b & 0x7f) << (7 * i);
                if b < 0x80 {
                    return Ok(out);
                }
            }
            Err("bond: varint too long".into())
        }

        /// A count of things that each take at least one byte.
        fn count(&mut self) -> Result<usize, String> {
            let n = self.varint()?;
            if n > (self.data.len() - self.pos) as u64 {
                return Err("bond: count past the end".into());
            }
            Ok(n as usize)
        }

        fn fields(&mut self, depth: usize) -> Result<Vec<Item>, String> {
            if depth > MAX_DEPTH {
                return Err("bond: nested too deep".into());
            }
            let mut items = Vec::new();
            loop {
                let head = self.byte()?;
                let ty = head & 0x1f;
                let id = match head >> 5 {
                    6 => u16::from(self.byte()?),
                    7 => u16::from_le_bytes([self.byte()?, self.byte()?]),
                    small => u16::from(small),
                };
                match ty {
                    STOP => return Ok(items),
                    STOP_BASE => items.push(Item::Base),
                    _ => {
                        let value = self.value(ty, depth)?;
                        items.push(Item::Field { id, ty, value });
                    }
                }
            }
        }

        fn value(&mut self, ty: u8, depth: usize) -> Result<Value, String> {
            Ok(match ty {
                BOOL | UINT8 | INT8 => Value::Byte(self.byte()?),
                UINT16 | UINT32 | UINT64 | INT16 | INT32 | INT64 => Value::Varint(self.varint()?),
                FLOAT => Value::Fixed(self.bytes(4)?),
                DOUBLE => Value::Fixed(self.bytes(8)?),
                STRING => {
                    let n = self.count()?;
                    Value::Text(self.bytes(n)?)
                }
                WSTRING => {
                    let n = self.count()?;
                    Value::Text(self.bytes(n.checked_mul(2).ok_or("bond: wstring too long")?)?)
                }
                STRUCT => Value::Struct(self.fields(depth + 1)?),
                LIST | SET => {
                    let elem = self.byte()?;
                    let n = self.count()?;
                    let mut items = Vec::with_capacity(n);
                    for _ in 0..n {
                        items.push(self.value(elem & 0x1f, depth + 1)?);
                    }
                    Value::List { elem, items }
                }
                MAP => {
                    let (key, value) = (self.byte()?, self.byte()?);
                    let n = self.count()?;
                    let mut pairs = Vec::with_capacity(n);
                    for _ in 0..n {
                        let k = self.value(key & 0x1f, depth + 1)?;
                        let v = self.value(value & 0x1f, depth + 1)?;
                        pairs.push((k, v));
                    }
                    Value::Map { key, value, pairs }
                }
                other => return Err(format!("bond: unknown type {other}")),
            })
        }
    }

    /// A marshaled struct (`CB` header, fields, stop), all bytes used.
    pub fn decode(data: &[u8]) -> Result<Vec<Item>, String> {
        if !data.starts_with(&MAGIC) {
            return Err("bond: not CompactBinary v1".into());
        }
        let mut r = Reader { data, pos: MAGIC.len() };
        let items = r.fields(0)?;
        if r.pos != data.len() {
            return Err("bond: bytes after the struct".into());
        }
        Ok(items)
    }

    pub fn encode(items: &[Item]) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        write_fields(&mut out, items);
        out
    }

    fn varint(out: &mut Vec<u8>, mut v: u64) {
        loop {
            let b = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                out.push(b);
                return;
            }
            out.push(b | 0x80);
        }
    }

    fn write_fields(out: &mut Vec<u8>, items: &[Item]) {
        for item in items {
            match item {
                Item::Base => out.push(STOP_BASE),
                Item::Field { id, ty, value } => {
                    match *id {
                        0..=5 => out.push((*id as u8) << 5 | ty),
                        6..=255 => out.extend([0xc0 | ty, *id as u8]),
                        _ => {
                            out.push(0xe0 | ty);
                            out.extend(id.to_le_bytes());
                        }
                    }
                    write_value(out, *ty, value);
                }
            }
        }
        out.push(STOP);
    }

    fn write_value(out: &mut Vec<u8>, ty: u8, value: &Value) {
        match value {
            Value::Byte(b) => out.push(*b),
            Value::Varint(v) => varint(out, *v),
            Value::Fixed(b) => out.extend(b),
            Value::Text(b) => {
                varint(out, if ty & 0x1f == WSTRING { b.len() as u64 / 2 } else { b.len() as u64 });
                out.extend(b);
            }
            Value::Struct(items) => write_fields(out, items),
            Value::List { elem, items } => {
                out.push(*elem);
                varint(out, items.len() as u64);
                for v in items {
                    write_value(out, *elem, v);
                }
            }
            Value::Map { key, value, pairs } => {
                out.extend([*key, *value]);
                varint(out, pairs.len() as u64);
                for (k, v) in pairs {
                    write_value(out, *key, k);
                    write_value(out, *value, v);
                }
            }
        }
    }

    pub fn field(items: &[Item], want: u16) -> Option<(u8, &Value)> {
        items.iter().find_map(|i| match i {
            Item::Field { id, ty, value } if *id == want => Some((*ty, value)),
            _ => None,
        })
    }

    pub fn field_mut(items: &mut [Item], want: u16) -> Option<(u8, &mut Value)> {
        items.iter_mut().find_map(|i| match i {
            Item::Field { id, ty, value } if *id == want => Some((*ty, value)),
            _ => None,
        })
    }

    /// Replace a field, or insert it in ascending id order.
    pub fn set(items: &mut Vec<Item>, id: u16, ty: u8, value: Value) {
        let new = Item::Field { id, ty, value };
        if let Some(at) = items.iter().position(|i| matches!(i, Item::Field { id: x, .. } if *x == id)) {
            items[at] = new;
            return;
        }
        let at = items
            .iter()
            .position(|i| matches!(i, Item::Field { id: x, .. } if *x > id))
            .unwrap_or(items.len());
        items.insert(at, new);
    }

    pub fn remove(items: &mut Vec<Item>, id: u16) {
        items.retain(|i| !matches!(i, Item::Field { id: x, .. } if *x == id));
    }
}

use bond::{Item, Value as Bond};

/// Inner state fields.
const ON: u16 = 0;
const CAUSE: u16 = 10;
const CHANGED: u16 = 20;
/// FILETIME of the Unix epoch, in 100 ns ticks.
const FILETIME_UNIX: u64 = 11_644_473_600 * 10_000_000;

/// A state blob taken apart: the envelope, the inner struct, and whether
/// both re-encode to exactly what was read.
#[derive(Debug)]
struct Parsed {
    outer: Vec<Item>,
    inner: Vec<Item>,
    exact: bool,
}

/// Envelope: outer field 1 (payload container).
fn container(outer: &mut [Item]) -> Option<&mut Vec<Item>> {
    match bond::field_mut(outer, 1)? {
        (bond::STRUCT, Bond::Struct(items)) => Some(items),
        _ => None,
    }
}

/// Envelope: the inner payload's bytes, `container.1.1: list<int8>`.
fn payload_list(outer: &mut [Item]) -> Option<&mut Vec<Bond>> {
    let wrapper = match bond::field_mut(container(outer)?, 1)? {
        (bond::STRUCT, Bond::Struct(items)) => items,
        _ => return None,
    };
    match bond::field_mut(wrapper, 1)? {
        (bond::LIST, Bond::List { elem, items }) if *elem == bond::INT8 => Some(items),
        _ => None,
    }
}

fn parse(blob: &[u8]) -> Result<Parsed, String> {
    let mut outer = bond::decode(blob)?;
    let list = payload_list(&mut outer).ok_or("night light: no payload in the envelope")?;
    let payload: Vec<u8> = list
        .iter()
        .map(|v| match v {
            Bond::Byte(b) => Ok(*b),
            _ => Err("night light: payload is not bytes".to_string()),
        })
        .collect::<Result<_, _>>()?;
    let inner = bond::decode(&payload)?;
    let typed = |id, ty| bond::field(&inner, id).is_none_or(|(t, _)| t == ty);
    let exact = bond::encode(&outer) == blob
        && bond::encode(&inner) == payload
        // The fields this module writes must have the types it writes.
        && matches!(bond::field(&outer, 1).and_then(|(_, c)| match c {
            Bond::Struct(c) => bond::field(c, 0),
            _ => None,
        }), Some((bond::UINT64, Bond::Varint(_))))
        && typed(ON, bond::INT32)
        && typed(CAUSE, bond::INT32)
        && typed(CHANGED, bond::UINT64)
        && !inner.contains(&Item::Base);
    Ok(Parsed { outer, inner, exact })
}

/// `(on, settable)`: whether night light is on now, and whether this blob
/// may be rewritten.
pub fn state(blob: &[u8]) -> Result<(bool, bool), String> {
    let p = parse(blob)?;
    Ok((bond::field(&p.inner, ON).is_some(), p.exact))
}

/// The blob with night light switched `on` by hand at `now` (Unix seconds,
/// FILETIME): field 0 set or cleared, cause 1, change time, and the envelope
/// stamped `max(now, previous + 2)` as Windows' own Quick Settings does.
pub fn switched(blob: &[u8], on: bool, now_unix: u64, now_filetime: u64) -> Result<Vec<u8>, String> {
    let Parsed { mut outer, mut inner, exact } = parse(blob)?;
    if !exact {
        return Err("night light: this Windows stores the state in a layout Versailles does not know".into());
    }
    if on {
        bond::set(&mut inner, ON, bond::INT32, Bond::Varint(bond::zigzag(0)));
    } else {
        bond::remove(&mut inner, ON);
    }
    bond::set(&mut inner, CAUSE, bond::INT32, Bond::Varint(bond::zigzag(1)));
    bond::set(&mut inner, CHANGED, bond::UINT64, Bond::Varint(now_filetime));
    let payload = bond::encode(&inner);
    *payload_list(&mut outer).ok_or("night light: no payload")? = payload.into_iter().map(Bond::Byte).collect();
    let stamp = container(&mut outer).and_then(|c| bond::field_mut(c, 0)).ok_or("night light: no timestamp")?;
    if let (_, Bond::Varint(prev)) = stamp {
        *prev = now_unix.max(prev.saturating_add(2));
    }
    let out = bond::encode(&outer);
    // Belt and braces: what is written must read back as asked.
    match state(&out) {
        Ok((now_on, true)) if now_on == on => Ok(out),
        _ => Err("night light: the new state did not read back".into()),
    }
}

/// The envelope's Unix-seconds timestamp (for the live test).
#[cfg(test)]
fn stamp(blob: &[u8]) -> Option<u64> {
    let mut p = parse(blob).ok()?;
    match bond::field(container(&mut p.outer)?, 0)? {
        (_, Bond::Varint(v)) => Some(*v),
        _ => None,
    }
}

fn now() -> (u64, u64) {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    (t.as_secs(), FILETIME_UNIX + t.as_secs() * 10_000_000 + u64::from(t.subsec_nanos() / 100))
}

/// `{ supported, on }` for the `display` read. `on` is `null` when the state
/// cannot be read; `supported` is whether it can be switched.
pub fn read() -> Value {
    match imp::read_blob(imp::STATE_KEY).map(|b| state(&b)) {
        Some(Ok((on, settable))) => json!({ "supported": settable, "on": on }),
        _ => json!({ "supported": false, "on": null }),
    }
}

/// Switch night light on or off (by hand, as the Quick Settings tile does).
pub fn set(on: bool) -> crate::error::AppResult<()> {
    use crate::error::AppError;
    let blob = imp::read_blob(imp::STATE_KEY).ok_or_else(|| AppError::msg("night light: not supported on this Windows"))?;
    if state(&blob).map_err(AppError::msg)?.0 == on {
        return Ok(());
    }
    let (unix, filetime) = now();
    let new = switched(&blob, on, unix, filetime).map_err(AppError::msg)?;
    imp::write_blob(imp::STATE_KEY, &new)
}

#[cfg(windows)]
mod imp {
    use crate::error::{AppError, AppResult};
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Foundation::ERROR_MORE_DATA;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegGetValueW, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_BINARY,
        RRF_RT_REG_BINARY,
    };

    #[cfg(test)]
    pub const CURRENT: &str = r"Software\Microsoft\Windows\CurrentVersion\CloudStore\Store\DefaultAccount\Current";
    pub const STATE_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\CloudStore\Store\DefaultAccount\Current\default$windows.data.bluelightreduction.bluelightreductionstate\windows.data.bluelightreduction.bluelightreductionstate";

    /// `Data` under `HKCU\<path>`; `None` when missing or not binary.
    pub fn read_blob(path: &str) -> Option<Vec<u8>> {
        let path = HSTRING::from(path);
        for _ in 0..3 {
            let mut size = 0u32;
            let probe = unsafe {
                RegGetValueW(HKEY_CURRENT_USER, &path, windows::core::w!("Data"), RRF_RT_REG_BINARY, None, None, Some(&mut size))
            };
            if probe.is_err() || size == 0 || size > 64 * 1024 {
                return None;
            }
            let mut buf = vec![0u8; size as usize];
            let got = unsafe {
                RegGetValueW(
                    HKEY_CURRENT_USER,
                    &path,
                    windows::core::w!("Data"),
                    RRF_RT_REG_BINARY,
                    None,
                    Some(buf.as_mut_ptr().cast()),
                    Some(&mut size),
                )
            };
            if got == ERROR_MORE_DATA {
                continue; // grew between the two calls
            }
            if got.is_err() {
                return None;
            }
            buf.truncate(size as usize);
            return Some(buf);
        }
        None
    }

    /// Overwrite `Data` under an existing key (never creates one).
    pub fn write_blob(path: &str, data: &[u8]) -> AppResult<()> {
        let path = HSTRING::from(path);
        let mut key = HKEY::default();
        let opened = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(path.as_ptr()), None, KEY_SET_VALUE, &mut key) };
        if opened.is_err() {
            return Err(AppError::msg(format!("night light: cannot open the state key ({opened:?})")));
        }
        let wrote = unsafe { RegSetValueExW(key, windows::core::w!("Data"), None, REG_BINARY, Some(data)) };
        unsafe {
            let _ = RegCloseKey(key);
        }
        if wrote.is_err() {
            return Err(AppError::msg(format!("night light: Windows refused the write ({wrote:?})")));
        }
        Ok(())
    }

    /// Subkeys of `Current` (for the live test: the per-device state key).
    #[cfg(test)]
    pub fn current_subkeys() -> Vec<String> {
        use windows::Win32::System::Registry::{RegEnumKeyExW, KEY_READ};
        let mut out = Vec::new();
        let path = HSTRING::from(CURRENT);
        let mut key = HKEY::default();
        unsafe {
            if RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(path.as_ptr()), None, KEY_READ, &mut key).is_err() {
                return out;
            }
            for i in 0.. {
                let mut name = [0u16; 512];
                let mut len = name.len() as u32;
                let r = RegEnumKeyExW(key, i, Some(windows::core::PWSTR(name.as_mut_ptr())), &mut len, None, None, None, None);
                if r.is_err() {
                    break;
                }
                out.push(String::from_utf16_lossy(&name[..len as usize]));
            }
            let _ = RegCloseKey(key);
        }
        out
    }
}

#[cfg(not(windows))]
mod imp {
    use crate::error::{AppError, AppResult};
    pub const STATE_KEY: &str = "";
    pub fn read_blob(_: &str) -> Option<Vec<u8>> {
        None
    }
    pub fn write_blob(_: &str, _: &[u8]) -> AppResult<()> {
        Err(AppError::msg("night light is only available on Windows"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        s.split_whitespace().map(|b| u8::from_str_radix(b, 16).unwrap()).collect()
    }

    /// Wrap an inner payload in the envelope Windows uses.
    fn envelope(stamp: &[u8], inner: &[u8]) -> Vec<u8> {
        let mut out = hex("43 42 01 00 0A 02 01 00 2A 06");
        out.extend(stamp);
        out.extend(hex("2A 2B 0E"));
        out.push(inner.len() as u8);
        out.extend(inner);
        out.extend([0, 0, 0]);
        out
    }

    /// The spec's settings example (schedule on, set hours, 2790 K): 60 bytes.
    const SETTINGS: &str = "43 42 01 00 0A 02 01 00 2A 06 EC A0 F4 BE 06 2A 2B 0E 26 \
        43 42 01 00 02 01 C2 0A 00 CA 14 0E 01 2E 0F 00 CA 1E 00 CF 28 CC 2B \
        CA 32 0E 13 2E 17 00 CA 3C 0E 07 2E 0C 00 00 00 00 00";
    /// The spec's state example (off, changed by hand), inner payload only.
    const STATE_OFF: &str = "43 42 01 00 D0 0A 02 C6 14 B0 9F D1 E6 F8 9F AF EE 01 00";
    /// This machine's (Windows 11 25H2) default$ state: never switched by hand.
    const MACHINE_DEFAULT: &str = "43 42 01 00 0A 02 01 00 2A 06 B0 8D 83 D2 06 2A 2B 0E 05 43 42 01 00 00 00 00 00";
    /// This machine's per-device state: an empty struct with a base (`01 00`).
    const MACHINE_PER_DEVICE: &str = "43 42 01 00 0A 02 01 00 2A 06 FB 9A A1 C9 06 2A 2B 0E 06 43 42 01 00 01 00 00 00 00";
    /// A `Cloud` record: different layout, never written.
    const CLOUD: &str = "43 42 01 00 0A 00 26 F5 90 D5 D6 06 00";

    #[test]
    fn varints_and_zigzag() {
        assert_eq!(bond::zigzag(2790), 5580);
        assert_eq!(bond::zigzag(-2790), 5579);
        assert_eq!(bond::zigzag(1), 2);
        assert_eq!(bond::unzigzag(5580), 2790);
        assert_eq!(bond::unzigzag(1), -1);
        let items = vec![Item::Field { id: 300, ty: bond::UINT32, value: Bond::Varint(16384) }];
        assert_eq!(bond::encode(&items), hex("43 42 01 00 E5 2C 01 80 80 01 00"));
        assert_eq!(bond::decode(&bond::encode(&items)).unwrap(), items);
    }

    #[test]
    fn spec_examples_round_trip() {
        for blob in [hex(SETTINGS), hex(STATE_OFF), hex(MACHINE_DEFAULT), hex(MACHINE_PER_DEVICE), hex(CLOUD)] {
            let items = bond::decode(&blob).unwrap();
            assert_eq!(bond::encode(&items), blob);
        }
        // The settings: color temperature is field 40, int16 2790.
        let outer = bond::decode(&hex(SETTINGS)).unwrap();
        let mut outer2 = outer.clone();
        let list = payload_list(&mut outer2).unwrap();
        assert_eq!(list.len(), 38);
        let inner: Vec<u8> = list.iter().map(|v| if let Bond::Byte(b) = v { *b } else { 0 }).collect();
        let inner = bond::decode(&inner).unwrap();
        assert_eq!(bond::field(&inner, 40), Some((bond::INT16, &Bond::Varint(5580))));
        // The full-example struct from the Bond spec.
        let full = hex("43 42 01 00 02 01 D0 0A 54 CA 14 06 E8 07 00 00");
        let items = bond::decode(&full).unwrap();
        assert_eq!(bond::field(&items, 10), Some((bond::INT32, &Bond::Varint(84))));
        assert_eq!(bond::encode(&items), full);
    }

    #[test]
    fn reads_state() {
        assert_eq!(state(&hex(MACHINE_DEFAULT)).unwrap(), (false, true));
        assert_eq!(state(&envelope(&hex("B0 8D 83 D2 06"), &hex(STATE_OFF))).unwrap(), (false, true));
        let on = hex("43 42 01 00 10 00 D0 0A 02 C6 14 B0 9F D1 E6 F8 9F AF EE 01 00");
        assert_eq!(state(&envelope(&hex("B0 8D 83 D2 06"), &on)).unwrap(), (true, true));
        // Per-device has a base-class stop: readable, not written.
        assert_eq!(state(&hex(MACHINE_PER_DEVICE)).unwrap(), (false, false));
        // Not an envelope.
        assert!(state(&hex(CLOUD)).is_err());
    }

    #[test]
    fn switching_changes_only_the_state_fields() {
        let blob = envelope(&hex("B0 8D 83 D2 06"), &hex(STATE_OFF));
        let prev = stamp(&blob).unwrap();
        // Clock behind the stamp: previous + 2.
        let ft = 134_189_894_679_678_896 + 10_000_000;
        let on = switched(&blob, true, prev - 100, ft).unwrap();
        assert_eq!(state(&on).unwrap(), (true, true));
        assert_eq!(stamp(&on), Some(prev + 2));
        let mut parsed = parse(&on).unwrap();
        assert_eq!(bond::field(&parsed.inner, ON), Some((bond::INT32, &Bond::Varint(0))));
        assert_eq!(bond::field(&parsed.inner, CAUSE), Some((bond::INT32, &Bond::Varint(2))));
        assert_eq!(bond::field(&parsed.inner, CHANGED), Some((bond::UINT64, &Bond::Varint(ft))));
        // Field 0 comes first, the metadata is unchanged.
        assert_eq!(&bond::encode(&parsed.inner)[..6], &hex("43 42 01 00 10 00")[..]);
        assert_eq!(bond::field(&parsed.outer, 0), bond::field(&bond::decode(&blob).unwrap(), 0));
        assert!(payload_list(&mut parsed.outer).is_some());

        // Clock ahead: now. Off again restores the original inner layout.
        let off = switched(&on, false, prev + 1000, 134_189_894_679_678_896).unwrap();
        assert_eq!(state(&off).unwrap(), (false, true));
        assert_eq!(stamp(&off), Some(prev + 1000));
        let new_stamp = bond::encode(&[Item::Field { id: 0, ty: bond::UINT64, value: Bond::Varint(prev + 1000) }]);
        assert_eq!(off, envelope(&new_stamp[5..new_stamp.len() - 1], &hex(STATE_OFF)));
    }

    #[test]
    fn switching_the_untouched_machine_state() {
        // An empty inner struct gains field 0, cause and time.
        let on = switched(&hex(MACHINE_DEFAULT), true, 1_800_000_000, 1).unwrap();
        let p = parse(&on).unwrap();
        assert_eq!(bond::encode(&p.inner), hex("43 42 01 00 10 00 D0 0A 02 C6 14 01 00"));
    }

    #[test]
    fn refuses_what_it_cannot_reproduce() {
        // Field 0 written in the long header form: parses, re-encodes differently.
        let odd = envelope(&hex("B0 8D 83 D2 06"), &hex("43 42 01 00 D0 00 00 00"));
        assert_eq!(state(&odd).unwrap(), (true, false));
        assert!(switched(&odd, false, 1, 1).is_err());
        // Non-canonical varint (0 as `80 00`).
        let odd = envelope(&hex("B0 8D 83 D2 06"), &hex("43 42 01 00 10 80 00 00"));
        assert_eq!(state(&odd).unwrap(), (true, false));
        // Field 0 with another type.
        let odd = envelope(&hex("B0 8D 83 D2 06"), &hex("43 42 01 00 02 01 00"));
        assert_eq!(state(&odd).unwrap().1, false);
        // A base-class layout.
        assert!(switched(&hex(MACHINE_PER_DEVICE), true, 1, 1).is_err());
        // Garbage and truncation never panic.
        for bad in ["", "43 42 01 00", "43 42 02 00 00", "43 42 01 00 0B 0E FF FF FF FF 0F", "43 42 01 00 0A 0A 0A 0A"] {
            assert!(state(&hex(bad)).is_err(), "{bad}");
        }
        let full = hex(SETTINGS);
        for n in 0..full.len() {
            let _ = state(&full[..n]);
        }
        let deep = [hex("43 42 01 00"), vec![0x0a; 40]].concat();
        assert!(bond::decode(&deep).is_err());
    }

    /// `cargo test live_night_light -- --ignored --nocapture`: read-only. Prints
    /// the default$ and per-device state so they can be compared after
    /// switching night light by hand.
    #[test]
    #[ignore]
    fn live_night_light() {
        println!("read(): {}", read());
        let mut keys = vec![imp::STATE_KEY.to_string()];
        for sub in imp::current_subkeys() {
            if sub.ends_with("$windows.data.bluelightreduction.bluelightreductionstateperdevice") {
                let leaf = "windows.data.bluelightreduction.bluelightreductionstateperdevice";
                keys.push(format!(r"{}\{sub}\{leaf}", imp::CURRENT));
            }
        }
        for key in keys {
            let name = key.rsplit('\\').nth(1).unwrap_or(&key).to_string();
            match imp::read_blob(&key) {
                None => println!("{name}: missing"),
                Some(blob) => {
                    let hex: Vec<String> = blob.iter().map(|b| format!("{b:02X}")).collect();
                    println!("{name}: {}", hex.join(" "));
                    println!("  state (on, settable): {:?}, stamp: {:?}", state(&blob), stamp(&blob));
                    if let Ok(p) = parse(&blob) {
                        println!("  inner: {:?}", p.inner);
                    }
                }
            }
        }
    }
}
