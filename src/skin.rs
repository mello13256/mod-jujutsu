//! A pele do jogador, para o corpo que as técnicas posam: o nome que o jogo dá a ela no atlas,
//! tirado do perfil do jogador como o vanilla tira (`SkinManager`): a propriedade `textures` do
//! perfil é um JSON em base64 com o endereço da pele; o nome no atlas é `skins/` e o SHA-1 do
//! nome do arquivo do endereço. Sem pele no perfil (um jogador offline), a pele padrão do UUID.

/// A pele de um jogador: o nome no atlas, se o modelo é o fino, e a pele padrão do UUID dele
/// para quando a baixada ainda não chegou.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skin {
    pub sprite: String,
    pub slim: bool,
    pub fallback: &'static str,
}

/// A pele de quem tem este UUID e esta propriedade `textures` (o valor em base64).
pub fn of(uuid: u128, textures: Option<&str>) -> Skin {
    let fallback = default_skin(uuid);
    let downloaded = textures.and_then(unpack).and_then(|(url, slim)| Some((skin_location(&texture_hash(&url)?), slim)));
    match downloaded {
        Some((sprite, slim)) => Skin { sprite, slim, fallback },
        None => Skin { sprite: fallback.to_owned(), slim: fallback.contains("/slim/"), fallback },
    }
}

/// `DefaultPlayerSkin.get(uuid)`: uma das dezoito peles padrão pelo hash do UUID.
pub fn default_skin(uuid: u128) -> &'static str {
    const SKINS: [&str; 18] = [
        "entity/player/slim/alex",
        "entity/player/slim/ari",
        "entity/player/slim/efe",
        "entity/player/slim/kai",
        "entity/player/slim/makena",
        "entity/player/slim/noor",
        "entity/player/slim/steve",
        "entity/player/slim/sunny",
        "entity/player/slim/zuri",
        "entity/player/wide/alex",
        "entity/player/wide/ari",
        "entity/player/wide/efe",
        "entity/player/wide/kai",
        "entity/player/wide/makena",
        "entity/player/wide/noor",
        "entity/player/wide/steve",
        "entity/player/wide/sunny",
        "entity/player/wide/zuri",
    ];
    let hilo = ((uuid >> 64) as u64) ^ (uuid as u64);
    let hash = ((hilo >> 32) as i32) ^ (hilo as i32);
    SKINS[hash.rem_euclid(SKINS.len() as i32) as usize]
}

/// O nome do arquivo de um endereço, sem a extensão (`registerTexture`).
pub fn texture_hash(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let path = &rest[rest.find('/').unwrap_or(rest.len())..];
    let path = &path[..path.find(['?', '#']).unwrap_or(path.len())];
    let name = &path[path.rfind(['/', '\\']).map_or(0, |i| i + 1)..];
    Some(name[..name.rfind('.').unwrap_or(name.len())].to_owned())
}

/// `skins/` e o SHA-1 das unidades UTF-16 do nome (`Hashing.sha1().hashUnencodedChars`).
pub fn skin_location(hash: &str) -> String {
    let units: Vec<u8> = hash.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut out = String::from("skins/");
    for b in sha1(&units) {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// O endereço da pele e se o modelo é o fino, do valor da propriedade `textures`.
fn unpack(value: &str) -> Option<(String, bool)> {
    let json = String::from_utf8(base64(value)?).ok()?;
    // O objeto da pele: depois de "SKIN", até a chave que fecha o objeto dela.
    let skin = &json[json.find("\"SKIN\"")? + 6..];
    let url = string_after(skin, "\"url\"")?;
    let end = matching_brace(skin)?;
    let slim = string_after(&skin[..end], "\"model\"").is_some_and(|model| model == "slim");
    Some((url, slim))
}

/// O texto entre aspas depois de `key` e dos dois-pontos.
fn string_after(text: &str, key: &str) -> Option<String> {
    let rest = text[text.find(key)? + key.len()..].trim_start().strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => out.push(match chars.next()? {
                'n' => '\n',
                't' => '\t',
                other => other,
            }),
            c => out.push(c),
        }
    }
    None
}

/// Onde fecha o primeiro objeto `{...}` de `text`.
fn matching_brace(text: &str) -> Option<usize> {
    let start = text.find('{')?;
    let (mut depth, mut quoted, mut escaped) = (0i32, false, false);
    for (i, c) in text[start..].char_indices() {
        if quoted {
            match (escaped, c) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => quoted = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => quoted = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(start + i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Base64 padrão, com ou sem `=` no fim.
fn base64(text: &str) -> Option<Vec<u8>> {
    let value = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        } as u32)
    };
    let digits: Vec<u8> = text.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=').collect();
    let mut out = Vec::with_capacity(digits.len() * 3 / 4);
    for chunk in digits.chunks(4) {
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= value(c)? << (18 - 6 * i);
        }
        let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        out.extend_from_slice(&bytes[..chunk.len().saturating_sub(1)]);
    }
    Some(out)
}

/// SHA-1 (FIPS 180-4).
fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x6745_2301, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476, 0xC3D2_E1F0];
    let mut message = data.to_vec();
    let bits = (data.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bits.to_be_bytes());
    for block in message.chunks(64) {
        let mut w = [0u32; 80];
        for (i, word) in block.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, &wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let t = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e]) {
            *x = x.wrapping_add(y);
        }
    }
    let mut out = [0u8; 20];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn sha1_matches_the_standard_vectors() {
        assert_eq!(hex(&sha1(b"abc")), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(hex(&sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(
            hex(&sha1(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
    }

    #[test]
    fn a_profile_with_a_skin_names_its_atlas_sprite_and_model() {
        let json = r#"{"timestamp":1,"profileName":"Meln","textures":{"SKIN":{"url":"http://textures.minecraft.net/texture/abc123","metadata":{"model":"slim"}},"CAPE":{"url":"http://textures.minecraft.net/texture/cape9"}}}"#;
        let encoded = encode(json.as_bytes());
        let skin = of(7, Some(&encoded));
        // O SHA-1 dos caracteres UTF-16 de "abc123", em little-endian.
        let units: Vec<u8> = "abc123".encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(skin.sprite, format!("skins/{}", hex(&sha1(&units))));
        assert!(skin.slim);
        // Sem o model, é o largo; a capa não confunde.
        let wide = r#"{"textures":{"CAPE":{"url":"http://x/texture/c"},"SKIN":{"url":"http://x/texture/s.png"}}}"#;
        assert!(!of(7, Some(&encode(wide.as_bytes()))).slim);
        assert_eq!(texture_hash("http://x/texture/s.png?x=1").as_deref(), Some("s"));
    }

    #[test]
    fn without_a_skin_the_default_of_the_uuid() {
        let skin = of(0, None);
        assert_eq!(skin.sprite, default_skin(0));
        assert_eq!(skin.slim, skin.sprite.contains("/slim/"));
        assert_eq!(of(0, Some("não é base64")).sprite, default_skin(0));
    }

    fn encode(bytes: &[u8]) -> String {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let n = chunk.iter().enumerate().fold(0u32, |n, (i, &b)| n | u32::from(b) << (16 - 8 * i));
            for i in 0..=chunk.len() {
                out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
            }
        }
        while out.len() % 4 != 0 {
            out.push('=');
        }
        out
    }
}
