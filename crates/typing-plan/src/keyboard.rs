//! What the plan knows about a QWERTY keyboard: which keys sit next to each other (for typos)
//! and which finger types each key (for rhythm).

/// Keys around each letter key, used for realistic wrong-key typos.
pub fn neighbors(c: char) -> &'static str {
    match c {
        'q' => "wa",
        'w' => "qesa",
        'e' => "wrds",
        'r' => "etfd",
        't' => "rygf",
        'y' => "tuhg",
        'u' => "yijh",
        'i' => "uokj",
        'o' => "iplk",
        'p' => "ol",
        'a' => "qwsz",
        's' => "awedzx",
        'd' => "serfxc",
        'f' => "drtgcv",
        'g' => "ftyhvb",
        'h' => "gyujbn",
        'j' => "huiknm",
        'k' => "jiolm",
        'l' => "kop",
        'z' => "asx",
        'x' => "zsdc",
        'c' => "xdfv",
        'v' => "cfgb",
        'b' => "vghn",
        'n' => "bhjm",
        'm' => "njk",
        _ => "",
    }
}

/// Finger that types a key: 0..=3 left hand (pinky to index), 4..=7 right hand (index to pinky).
pub fn finger(c: char) -> Option<u8> {
    const COLUMNS: [&str; 8] = [
        "`1qaz",
        "2wsx",
        "3edc",
        "45rtfgvb",
        "67yuhjnm",
        "8ik,",
        "9ol.",
        "0p;/-=[]'\\",
    ];
    let c = c.to_ascii_lowercase();
    COLUMNS.iter().position(|col| col.contains(c)).map(|f| f as u8)
}

/// Letter pairs common enough in English that they roll off the fingers.
pub fn is_fast_bigram(a: char, b: char) -> bool {
    const PAIRS: &[&str] = &[
        "th", "he", "in", "er", "an", "re", "on", "at", "en", "nd", "ti", "es", "or", "te", "of",
        "ed", "is", "it", "al", "ar", "st", "to", "nt", "ng", "se", "ha", "as", "ou", "io", "le",
        "ve", "co", "me", "de", "hi", "ri", "ro", "ic", "ne", "ea", "ra", "ce",
    ];
    let pair = [a.to_ascii_lowercase(), b.to_ascii_lowercase()];
    PAIRS.iter().any(|p| p.chars().eq(pair.iter().copied()))
}

/// Needs Shift on a US layout.
pub fn needs_shift(c: char) -> bool {
    c.is_ascii_uppercase() || "~!@#$%^&*()_+{}|:\"<>?".contains(c)
}
