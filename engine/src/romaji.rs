//! IME 방식 로마자 입력 -> 히라가나 (ime_v0/romaji.py의 parse 이식).

use std::collections::HashMap;
use std::sync::OnceLock;

const BASE: &str = "
あa いi うu えe おo かka きki くku けke こko さsa しshi すsu せse そso たta ちchi つtsu てte とto
なna にni ぬnu ねne のno はha ひhi ふfu へhe ほho まma みmi むmu めme もmo やya ゆyu よyo
らra りri るru れre ろro わwa をwo がga ぎgi ぐgu げge ごgo ざza じji ずzu ぜze ぞzo
だda ぢdi づdu でde どdo ばba びbi ぶbu べbe ぼbo ぱpa ぴpi ぷpu ぺpe ぽpo ゔvu
きゃkya きゅkyu きょkyo しゃsha しゅshu しょsho ちゃcha ちゅchu ちょcho にゃnya にゅnyu にょnyo
ひゃhya ひゅhyu ひょhyo みゃmya みゅmyu みょmyo りゃrya りゅryu りょryo ぎゃgya ぎゅgyu ぎょgyo
じゃja じゅju じょjo びゃbya びゅbyu びょbyo ぴゃpya ぴゅpyu ぴょpyo ぢゃdya ぢゅdyu ぢょdyo
しぇshe ちぇche じぇje ふぁfa ふぃfi ふぇfe ふぉfo てぃthi でぃdhi でゅdhu うぃwi うぇwe うぉwho
ゔぁva ゔぃvi ゔぇve ゔぉvo つぁtsa
ぁxa ぃxi ぅxu ぇxe ぉxo ゃxya ゅxyu ょxyo ゎxwa ゐwi ゑwe
";

const EXTRA: &[(&str, &str)] = &[
    ("si", "し"), ("ti", "ち"), ("tu", "つ"), ("hu", "ふ"), ("zi", "じ"), ("di", "ぢ"), ("du", "づ"),
    ("sya", "しゃ"), ("syu", "しゅ"), ("syo", "しょ"), ("tya", "ちゃ"), ("tyu", "ちゅ"), ("tyo", "ちょ"),
    ("cya", "ちゃ"), ("cyu", "ちゅ"), ("cyo", "ちょ"), ("jya", "じゃ"), ("jyu", "じゅ"), ("jyo", "じょ"),
    ("zya", "じゃ"), ("zyu", "じゅ"), ("zyo", "じょ"), ("la", "ぁ"), ("li", "ぃ"), ("lu", "ぅ"), ("le", "ぇ"), ("lo", "ぉ"),
    ("xtu", "っ"), ("ltu", "っ"), ("xtsu", "っ"), ("lya", "ゃ"), ("lyu", "ゅ"), ("lyo", "ょ"), ("nn", "ん"), ("n'", "ん"),
    ("ca", "か"), ("cu", "く"), ("co", "こ"), ("qa", "くぁ"), ("ye", "いぇ"), ("wu", "う"),
];

struct Table {
    map: HashMap<String, String>,
    max_len: usize,
}

fn table() -> &'static Table {
    static T: OnceLock<Table> = OnceLock::new();
    T.get_or_init(|| {
        // Python: KANA_TO_ROMAJI(가나->로마자)를 순서대로 뒤집어 같은 로마자는 나중 것이 남는다 (wi -> ゐ, we -> ゑ)
        let mut map: HashMap<String, String> = HashMap::new();
        for item in BASE.split_whitespace() {
            let kana: String = item.chars().filter(|c| !c.is_ascii()).collect();
            let roma: String = item.chars().filter(|c| c.is_ascii()).collect();
            map.insert(roma, kana);
        }
        for (r, k) in EXTRA {
            map.insert(r.to_string(), k.to_string());
        }
        let max_len = map.keys().map(|k| k.chars().count()).max().unwrap_or(1);
        Table { map, max_len }
    })
}

fn is_vowel(c: char) -> bool {
    matches!(c, 'a' | 'i' | 'u' | 'e' | 'o')
}

#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub kana: String,
    /// 가나가 되지 못한 로마자 글자 수
    pub leftover: usize,
}

pub fn parse(keys: &str) -> Parsed {
    let t = table();
    let orig: Vec<char> = keys.chars().collect();
    let s: Vec<char> = keys.to_lowercase().chars().collect();
    let mut out = String::new();
    let mut left = 0;
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c == '-' {
            out.push('ー');
            i += 1;
            continue;
        }
        // 같은 자음 두 번 (n 제외) -> っ
        if i + 1 < s.len() && c == s[i + 1] && c.is_alphabetic() && !is_vowel(c) && c != 'n' {
            out.push('っ');
            i += 1;
            continue;
        }
        if c == 'n' && i + 1 < s.len() && !is_vowel(s[i + 1]) && !matches!(s[i + 1], 'y' | 'n' | '\'') {
            out.push('ん');
            i += 1;
            continue;
        }
        let mut matched = false;
        let maxl = t.max_len.min(s.len() - i);
        for l in (1..=maxl).rev() {
            let piece: String = s[i..i + l].iter().collect();
            if let Some(k) = t.map.get(&piece) {
                out.push_str(k);
                i += l;
                matched = true;
                break;
            }
        }
        if !matched {
            if c == 'n' && i == s.len() - 1 {
                out.push('ん');
            } else {
                out.push(orig.get(i).copied().unwrap_or(c));
                left += 1;
            }
            i += 1;
        }
    }
    Parsed { kana: out, leftover: left }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cases() {
        for (keys, kana) in [
            ("kyou", "きょう"), ("konnnichiha", "こんにちは"), ("konnichiha", "こんいちは"), ("shinbun", "しんぶん"),
            ("kitte", "きって"), ("sinbunn", "しんぶん"), ("ko-hi-", "こーひー"), ("tukutta", "つくった"),
        ] {
            let p = parse(keys);
            assert_eq!(p.kana, kana, "{keys}");
            assert_eq!(p.leftover, 0, "{keys}");
        }
        assert!(parse("strength").leftover > 0);
    }
}
