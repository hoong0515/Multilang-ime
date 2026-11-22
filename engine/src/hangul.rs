//! 두벌식 오토마타: QWERTY 키 입력 <-> 한글. (ime_v0/hangul.py 이식)

const CHO: [char; 19] = ['ㄱ', 'ㄲ', 'ㄴ', 'ㄷ', 'ㄸ', 'ㄹ', 'ㅁ', 'ㅂ', 'ㅃ', 'ㅅ', 'ㅆ', 'ㅇ', 'ㅈ', 'ㅉ', 'ㅊ', 'ㅋ', 'ㅌ', 'ㅍ', 'ㅎ'];
const JUNG: [char; 21] = [
    'ㅏ', 'ㅐ', 'ㅑ', 'ㅒ', 'ㅓ', 'ㅔ', 'ㅕ', 'ㅖ', 'ㅗ', 'ㅘ', 'ㅙ', 'ㅚ', 'ㅛ', 'ㅜ', 'ㅝ', 'ㅞ', 'ㅟ', 'ㅠ', 'ㅡ', 'ㅢ', 'ㅣ',
];
const JONG: [Option<char>; 28] = [
    None, Some('ㄱ'), Some('ㄲ'), Some('ㄳ'), Some('ㄴ'), Some('ㄵ'), Some('ㄶ'), Some('ㄷ'), Some('ㄹ'), Some('ㄺ'),
    Some('ㄻ'), Some('ㄼ'), Some('ㄽ'), Some('ㄾ'), Some('ㄿ'), Some('ㅀ'), Some('ㅁ'), Some('ㅂ'), Some('ㅄ'), Some('ㅅ'),
    Some('ㅆ'), Some('ㅇ'), Some('ㅈ'), Some('ㅊ'), Some('ㅋ'), Some('ㅌ'), Some('ㅍ'), Some('ㅎ'),
];

const VOWEL_COMB: [(char, char, char); 7] = [
    ('ㅗ', 'ㅏ', 'ㅘ'), ('ㅗ', 'ㅐ', 'ㅙ'), ('ㅗ', 'ㅣ', 'ㅚ'), ('ㅜ', 'ㅓ', 'ㅝ'), ('ㅜ', 'ㅔ', 'ㅞ'), ('ㅜ', 'ㅣ', 'ㅟ'), ('ㅡ', 'ㅣ', 'ㅢ'),
];
const FINAL_COMB: [(char, char, char); 11] = [
    ('ㄱ', 'ㅅ', 'ㄳ'), ('ㄴ', 'ㅈ', 'ㄵ'), ('ㄴ', 'ㅎ', 'ㄶ'), ('ㄹ', 'ㄱ', 'ㄺ'), ('ㄹ', 'ㅁ', 'ㄻ'), ('ㄹ', 'ㅂ', 'ㄼ'),
    ('ㄹ', 'ㅅ', 'ㄽ'), ('ㄹ', 'ㅌ', 'ㄾ'), ('ㄹ', 'ㅍ', 'ㄿ'), ('ㄹ', 'ㅎ', 'ㅀ'), ('ㅂ', 'ㅅ', 'ㅄ'),
];

/// 키 하나를 자모로. Shift가 의미 없는 대문자는 소문자와 같다.
pub fn key_to_jamo(k: char) -> Option<char> {
    Some(match k {
        'R' => 'ㄲ', 'E' => 'ㄸ', 'Q' => 'ㅃ', 'T' => 'ㅆ', 'W' => 'ㅉ', 'O' => 'ㅒ', 'P' => 'ㅖ',
        _ => match k.to_ascii_lowercase() {
            'r' => 'ㄱ', 's' => 'ㄴ', 'e' => 'ㄷ', 'f' => 'ㄹ', 'a' => 'ㅁ', 'q' => 'ㅂ', 't' => 'ㅅ', 'd' => 'ㅇ',
            'w' => 'ㅈ', 'c' => 'ㅊ', 'z' => 'ㅋ', 'x' => 'ㅌ', 'v' => 'ㅍ', 'g' => 'ㅎ',
            'k' => 'ㅏ', 'o' => 'ㅐ', 'i' => 'ㅑ', 'j' => 'ㅓ', 'p' => 'ㅔ', 'u' => 'ㅕ', 'h' => 'ㅗ', 'y' => 'ㅛ',
            'n' => 'ㅜ', 'b' => 'ㅠ', 'm' => 'ㅡ', 'l' => 'ㅣ',
            _ => return None,
        },
    })
}

fn jamo_to_key(j: char) -> Option<char> {
    Some(match j {
        'ㄲ' => 'R', 'ㄸ' => 'E', 'ㅃ' => 'Q', 'ㅆ' => 'T', 'ㅉ' => 'W', 'ㅒ' => 'O', 'ㅖ' => 'P',
        'ㄱ' => 'r', 'ㄴ' => 's', 'ㄷ' => 'e', 'ㄹ' => 'f', 'ㅁ' => 'a', 'ㅂ' => 'q', 'ㅅ' => 't', 'ㅇ' => 'd',
        'ㅈ' => 'w', 'ㅊ' => 'c', 'ㅋ' => 'z', 'ㅌ' => 'x', 'ㅍ' => 'v', 'ㅎ' => 'g',
        'ㅏ' => 'k', 'ㅐ' => 'o', 'ㅑ' => 'i', 'ㅓ' => 'j', 'ㅔ' => 'p', 'ㅕ' => 'u', 'ㅗ' => 'h', 'ㅛ' => 'y',
        'ㅜ' => 'n', 'ㅠ' => 'b', 'ㅡ' => 'm', 'ㅣ' => 'l',
        _ => return None,
    })
}

pub fn is_vowel(j: char) -> bool {
    JUNG.contains(&j)
}

fn vowel_comb(a: char, b: char) -> Option<char> {
    VOWEL_COMB.iter().find(|(x, y, _)| *x == a && *y == b).map(|t| t.2)
}

fn final_comb(a: char, b: char) -> Option<char> {
    FINAL_COMB.iter().find(|(x, y, _)| *x == a && *y == b).map(|t| t.2)
}

fn final_split(c: char) -> Option<(char, char)> {
    FINAL_COMB.iter().find(|t| t.2 == c).map(|t| (t.0, t.1))
}

fn vowel_split(c: char) -> Option<(char, char)> {
    VOWEL_COMB.iter().find(|t| t.2 == c).map(|t| (t.0, t.1))
}

fn can_be_final(j: char) -> bool {
    JONG.iter().any(|x| *x == Some(j))
}

fn syllable(cho: char, jung: char, jong: Option<char>) -> char {
    let ci = CHO.iter().position(|&c| c == cho).unwrap_or(0) as u32;
    let vi = JUNG.iter().position(|&c| c == jung).unwrap_or(0) as u32;
    let fi = JONG.iter().position(|&c| c == jong).unwrap_or(0) as u32;
    char::from_u32(0xAC00 + (ci * 21 + vi) * 28 + fi).unwrap_or('?')
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Composed {
    pub text: String,
    pub syllables: usize,
    pub orphan_cons: usize,
    pub orphan_vowels: usize,
    pub other: usize,
    pub orphan_list: Vec<char>,
}

impl Composed {
    pub fn orphans(&self) -> usize {
        self.orphan_cons + self.orphan_vowels
    }
}

pub fn compose(keys: &str) -> Composed {
    let mut res = Composed::default();
    let (mut cho, mut jung, mut jong): (Option<char>, Option<char>, Option<char>) = (None, None, None);

    let flush = |res: &mut Composed, cho: &mut Option<char>, jung: &mut Option<char>, jong: &mut Option<char>| {
        match (*cho, *jung) {
            (Some(c), Some(v)) => {
                res.text.push(syllable(c, v, *jong));
                res.syllables += 1;
            }
            (Some(c), None) => {
                res.text.push(c);
                res.orphan_cons += 1;
                res.orphan_list.push(c);
            }
            (None, Some(v)) => {
                res.text.push(v);
                res.orphan_vowels += 1;
                res.orphan_list.push(v);
            }
            (None, None) => {}
        }
        *cho = None;
        *jung = None;
        *jong = None;
    };

    for ch in keys.chars() {
        let Some(j) = key_to_jamo(ch) else {
            flush(&mut res, &mut cho, &mut jung, &mut jong);
            res.text.push(ch);
            res.other += 1;
            continue;
        };
        if !is_vowel(j) {
            if let Some(f) = jong {
                if let Some(c) = final_comb(f, j) {
                    jong = Some(c);
                } else {
                    flush(&mut res, &mut cho, &mut jung, &mut jong);
                    cho = Some(j);
                }
            } else if cho.is_some() && jung.is_some() && can_be_final(j) {
                jong = Some(j);
            } else {
                flush(&mut res, &mut cho, &mut jung, &mut jong);
                cho = Some(j);
            }
        } else if let Some(f) = jong {
            let moved = if let Some((keep, mv)) = final_split(f) {
                jong = Some(keep);
                mv
            } else {
                jong = None;
                f
            };
            flush(&mut res, &mut cho, &mut jung, &mut jong);
            cho = Some(moved);
            jung = Some(j);
        } else if let Some(v) = jung {
            if let Some(c) = vowel_comb(v, j) {
                jung = Some(c);
            } else {
                flush(&mut res, &mut cho, &mut jung, &mut jong);
                jung = Some(j);
            }
        } else {
            jung = Some(j);
        }
    }
    flush(&mut res, &mut cho, &mut jung, &mut jong);
    res
}

/// 한 글자를 키 입력 순서의 자모 목록으로.
fn decompose_char(ch: char) -> Vec<char> {
    let code = ch as u32;
    if (0xAC00..0xAC00 + 11172).contains(&code) {
        let s = code - 0xAC00;
        let (cho, rest) = (s / (21 * 28), s % (21 * 28));
        let (jung, jong) = (rest / 28, rest % 28);
        let mut parts = vec![CHO[cho as usize]];
        let v = JUNG[jung as usize];
        match vowel_split(v) {
            Some((a, b)) => parts.extend([a, b]),
            None => parts.push(v),
        }
        if let Some(f) = JONG[jong as usize] {
            match final_split(f) {
                Some((a, b)) => parts.extend([a, b]),
                None => parts.push(f),
            }
        }
        return parts;
    }
    if let Some((a, b)) = vowel_split(ch) {
        return vec![a, b];
    }
    if let Some((a, b)) = final_split(ch) {
        return vec![a, b];
    }
    if jamo_to_key(ch).is_some() {
        return vec![ch];
    }
    vec![]
}

pub fn is_hangul(ch: char) -> bool {
    let c = ch as u32;
    (0xAC00..=0xD7A3).contains(&c) || (0x3131..=0x3163).contains(&c)
}

pub fn to_keys(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        let parts = decompose_char(ch);
        if parts.is_empty() {
            out.push(ch);
        } else {
            out.extend(parts.into_iter().filter_map(jamo_to_key));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compose_cases() {
        for (keys, text) in [
            ("dkssudgktpdy", "안녕하세요"), ("rkqt", "값"), ("rkqtdl", "값이"), ("dhkswjs", "완전"), ("Tkd", "쌍"),
            ("zzz", "ㅋㅋㅋ"), ("bb", "ㅠㅠ"), ("gks", "한"), ("gksrmf", "한글"), ("dmlrmf", "의글"), ("ekfr", "닭"),
            ("ekfrdms", "닭은"), ("APIfmf", "몌ㅑ를"),
        ] {
            assert_eq!(compose(keys).text, text, "{keys}");
        }
    }

    #[test]
    fn hello_is_broken_korean() {
        let r = compose("hello");
        assert_eq!(r.text, "ㅗ디ㅣㅐ");
        assert_eq!((r.syllables, r.orphans()), (1, 3));
    }

    #[test]
    fn roundtrip() {
        for t in ["안녕하세요", "값이", "완전", "닭은", "쌍쌍바", "ㅋㅋㅋ", "ㅠㅠ", "읽었다", "괜찮아", "뷁"] {
            assert_eq!(compose(&to_keys(t)).text, t);
        }
    }
}
