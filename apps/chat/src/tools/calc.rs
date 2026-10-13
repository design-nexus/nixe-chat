//! A small, safe arithmetic evaluator for the calculator tool.

pub fn eval(expr: &str) -> Result<f64, String> {
    let tokens = lex(expr)?;
    let mut p = Parser { t: &tokens, i: 0 };
    let v = p.expr()?;
    if p.i != tokens.len() {
        return Err(format!("unexpected '{}'", tokens[p.i].show()));
    }
    if v.is_nan() {
        return Err("the result isn't a number".into());
    }
    Ok(v)
}

pub fn format(v: f64) -> String {
    if v.is_infinite() {
        return if v > 0.0 { "infinity".into() } else { "-infinity".into() };
    }
    if v.fract() == 0.0 && v.abs() < 1e15 {
        return format!("{}", v as i64);
    }
    let s = format!("{v:.12}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Ident(String),
    Op(char),
}

impl Tok {
    fn show(&self) -> String {
        match self {
            Tok::Num(n) => n.to_string(),
            Tok::Ident(s) => s.clone(),
            Tok::Op(c) => c.to_string(),
        }
    }
}

fn lex(s: &str) -> Result<Vec<Tok>, String> {
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || c == '.' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.' || chars[i] == '_') {
                i += 1;
            }
            if i < chars.len()
                && (chars[i] == 'e' || chars[i] == 'E')
                && chars.get(i + 1).is_some_and(|n| n.is_ascii_digit() || *n == '-' || *n == '+')
            {
                i += 2;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let text: String = chars[start..i].iter().filter(|c| **c != '_').collect();
            out.push(Tok::Num(text.parse().map_err(|_| format!("bad number '{text}'"))?));
        } else if c.is_alphabetic() {
            let start = i;
            while i < chars.len() && chars[i].is_alphanumeric() {
                i += 1;
            }
            out.push(Tok::Ident(chars[start..i].iter().collect::<String>().to_lowercase()));
        } else if "+-*/%^()×÷".contains(c) {
            out.push(Tok::Op(match c {
                '×' => '*',
                '÷' => '/',
                c => c,
            }));
            i += 1;
        } else {
            return Err(format!("unexpected '{c}'"));
        }
    }
    Ok(out)
}

struct Parser<'a> {
    t: &'a [Tok],
    i: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.t.get(self.i)
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(&Tok::Op(c)) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn expr(&mut self) -> Result<f64, String> {
        let mut v = self.term()?;
        loop {
            if self.eat('+') {
                v += self.term()?;
            } else if self.eat('-') {
                v -= self.term()?;
            } else {
                return Ok(v);
            }
        }
    }

    fn term(&mut self) -> Result<f64, String> {
        let mut v = self.unary()?;
        loop {
            if self.eat('*') {
                v *= self.unary()?;
            } else if self.eat('/') {
                let d = self.unary()?;
                if d == 0.0 {
                    return Err("division by zero".into());
                }
                v /= d;
            } else if self.eat('%') {
                v %= self.unary()?;
            } else {
                return Ok(v);
            }
        }
    }

    fn unary(&mut self) -> Result<f64, String> {
        if self.eat('-') {
            return Ok(-self.unary()?);
        }
        if self.eat('+') {
            return self.unary();
        }
        self.power()
    }

    fn power(&mut self) -> Result<f64, String> {
        let base = self.atom()?;
        if self.eat('^') {
            // Right-associative, and binds tighter than a leading minus on the exponent.
            let exp = self.unary()?;
            return Ok(base.powf(exp));
        }
        Ok(base)
    }

    fn atom(&mut self) -> Result<f64, String> {
        match self.peek().cloned() {
            Some(Tok::Num(n)) => {
                self.i += 1;
                Ok(n)
            }
            Some(Tok::Op('(')) => {
                self.i += 1;
                let v = self.expr()?;
                if !self.eat(')') {
                    return Err("missing ')'".into());
                }
                Ok(v)
            }
            Some(Tok::Ident(name)) => {
                self.i += 1;
                match name.as_str() {
                    "pi" => return Ok(std::f64::consts::PI),
                    "e" => return Ok(std::f64::consts::E),
                    _ => {}
                }
                if !self.eat('(') {
                    return Err(format!("unknown name '{name}'"));
                }
                let x = self.expr()?;
                if !self.eat(')') {
                    return Err("missing ')'".into());
                }
                Ok(match name.as_str() {
                    "sqrt" => x.sqrt(),
                    "sin" => x.sin(),
                    "cos" => x.cos(),
                    "tan" => x.tan(),
                    "asin" => x.asin(),
                    "acos" => x.acos(),
                    "atan" => x.atan(),
                    "ln" => x.ln(),
                    "log" => x.log10(),
                    "log2" => x.log2(),
                    "exp" => x.exp(),
                    "abs" => x.abs(),
                    "round" => x.round(),
                    "floor" => x.floor(),
                    "ceil" => x.ceil(),
                    _ => return Err(format!("unknown function '{name}'")),
                })
            }
            Some(t) => Err(format!("unexpected '{}'", t.show())),
            None => Err("the expression ended early".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic() {
        assert_eq!(eval("1 + 2 * 3").unwrap(), 7.0);
        assert_eq!(eval("(1 + 2) * 3").unwrap(), 9.0);
        assert_eq!(eval("2^3^2").unwrap(), 512.0);
        assert_eq!(eval("-2^2").unwrap(), -4.0);
        assert_eq!(eval("10 % 4").unwrap(), 2.0);
        assert_eq!(eval("sqrt(16) + abs(-2)").unwrap(), 6.0);
        assert_eq!(eval("1_000 * 3").unwrap(), 3000.0);
        assert_eq!(eval("1.5e3").unwrap(), 1500.0);
        assert!((eval("sin(pi/2)").unwrap() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn errors() {
        assert!(eval("1 / 0").is_err());
        assert!(eval("2 +").is_err());
        assert!(eval("foo(2)").is_err());
        assert!(eval("rm -rf").is_err());
    }

    #[test]
    fn formatting() {
        assert_eq!(format(3.0), "3");
        assert_eq!(format(0.1 + 0.2), "0.3");
        assert_eq!(format(1.0 / 3.0), "0.333333333333");
    }
}
