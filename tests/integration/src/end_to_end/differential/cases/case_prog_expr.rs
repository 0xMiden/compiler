// A tokenizer / expression evaluator over a `&str` the program builds from
// its inputs (campaign 27, program 1): the input pair is rendered into a byte
// buffer as a small arithmetic expression with spaces and a planted
// malformation, validated with `core::str::from_utf8`, split on `+` with
// `str::split`, each term trimmed and split again on `*`, and each factor
// parsed with `str::parse::<i32>` — the real `core::str` pattern machinery
// (`Split`, `CharSearcher`, `trim`, `char_indices`, `is_ascii_digit`) and
// `core::num`'s `from_str` state machine running on the VM. Errors travel in
// a derived-`Debug` enum through `?`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExprErr {
    Empty,
    NotUtf8(usize),
    BadDigit { at: usize, byte: u8 },
    Overflow,
    TooManyTerms(usize),
}

// Render `a op b op c` into `buf` with irregular spacing, and plant a
// non-digit byte in one term when `bad` selects it.
fn render(buf: &mut [u8; 96], a: u32, b: u32, bad: u32) -> usize {
    let mut n = 0usize;
    let mut write_num = |buf: &mut [u8; 96], n: &mut usize, mut v: u32| {
        let mut tmp = [0u8; 10];
        let mut k = 0usize;
        loop {
            tmp[k] = b'0' + (v % 10) as u8;
            v /= 10;
            k += 1;
            if v == 0 {
                break;
            }
        }
        while k > 0 {
            k -= 1;
            if *n < 96 {
                buf[*n] = tmp[k];
                *n += 1;
            }
        }
    };
    write_num(buf, &mut n, a % 10_000);
    for (i, sep) in [" + ", "*", " +"].iter().enumerate() {
        for &c in sep.as_bytes() {
            if n < 96 {
                buf[n] = c;
                n += 1;
            }
        }
        let v = match i {
            0 => b % 1_000,
            1 => (a ^ b) % 100,
            _ => (a.wrapping_add(b)) % 10_000,
        };
        write_num(buf, &mut n, v);
    }
    if bad % 5 == 1 && n > 2 {
        // A stray letter inside a term: `parse` must report it, not panic.
        buf[(bad as usize) % n] = b'x';
    }
    if bad % 5 == 2 && n > 3 {
        // An invalid UTF-8 lead byte, so `from_utf8` fails instead.
        buf[(bad as usize) % n] = 0xf5;
    }
    n
}

fn eval(text: &str) -> Result<i32, ExprErr> {
    if text.is_empty() {
        return Err(ExprErr::Empty);
    }
    let mut total = 0i32;
    let mut terms = 0usize;
    for term in text.split('+') {
        terms += 1;
        if terms > 4 {
            return Err(ExprErr::TooManyTerms(terms));
        }
        let mut product = 1i32;
        for factor in term.trim().split('*') {
            let f = factor.trim();
            // Locate the first non-digit ourselves, so the error carries a
            // position `parse` would not report.
            for (i, c) in f.char_indices() {
                if !c.is_ascii_digit() {
                    return Err(ExprErr::BadDigit {
                        at: i,
                        byte: f.as_bytes()[i],
                    });
                }
            }
            let v: i32 = f.parse().map_err(|_| ExprErr::Overflow)?;
            product = product.checked_mul(v).ok_or(ExprErr::Overflow)?;
        }
        total = total.checked_add(product).ok_or(ExprErr::Overflow)?;
    }
    Ok(total)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut buf = [0u8; 96];
    let n = render(&mut buf, input1, input2, input2 >> 3);
    let outcome = match core::str::from_utf8(&buf[..n]) {
        Ok(text) => eval(text.trim()),
        Err(e) => Err(ExprErr::NotUtf8(e.valid_up_to())),
    };
    match outcome {
        Ok(v) => (v as u32) ^ 0x1000_0000,
        Err(ExprErr::Empty) => 1,
        Err(ExprErr::NotUtf8(k)) => 0x2000_0000 | k as u32,
        Err(ExprErr::BadDigit { at, byte }) => 0x3000_0000 | ((at as u32) << 8) | byte as u32,
        Err(ExprErr::Overflow) => 0x4000_0000,
        Err(ExprErr::TooManyTerms(t)) => 0x5000_0000 | t as u32,
    }
    .wrapping_add(n as u32)
}
