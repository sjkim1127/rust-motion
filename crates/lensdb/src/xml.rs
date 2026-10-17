//! A small, bounded XML tree reader for lensfun-format database files.

use quick_xml::events::Event;

use crate::Error;

const MAX_NODES: usize = 500_000;
const MAX_DEPTH: usize = 32;

#[derive(Debug, Default)]
pub(crate) struct Node {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub text: String,
    pub children: Vec<Node>,
}

impl Node {
    pub fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }
    pub fn num(&self, k: &str) -> Option<f64> {
        self.attr(k).and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| v.is_finite())
    }
    pub fn kids<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }
    pub fn kid(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }
    pub fn kid_text(&self, name: &str) -> Option<&str> {
        self.kid(name).map(|n| n.text.trim()).filter(|t| !t.is_empty())
    }
}

fn entity(name: &str) -> Option<char> {
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        _ => {
            let n = name.strip_prefix('#')?;
            let v = match n.strip_prefix('x') {
                Some(h) => u32::from_str_radix(h, 16).ok()?,
                None => n.parse().ok()?,
            };
            char::from_u32(v)?
        }
    })
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(rest.get(..i).unwrap_or_default());
        let tail = rest.get(i..).unwrap_or_default();
        match tail.find(';').and_then(|j| entity(tail.get(1..j)?).map(|c| (c, j))) {
            Some((c, j)) => {
                out.push(c);
                rest = tail.get(j + 1..).unwrap_or_default();
            }
            None => {
                out.push('&');
                rest = tail.get(1..).unwrap_or_default();
            }
        }
    }
    out.push_str(rest);
    out
}

pub(crate) fn parse(s: &str) -> Result<Node, Error> {
    let mut reader = quick_xml::Reader::from_str(s);
    reader.config_mut().check_end_names = false;
    let mut stack = vec![Node { name: "#root".into(), ..Default::default() }];
    let mut count = 0usize;
    loop {
        let ev = reader.read_event().map_err(|e| Error::Xml(e.to_string()))?;
        match ev {
            Event::Start(ref e) | Event::Empty(ref e) => {
                count += 1;
                if count > MAX_NODES || stack.len() > MAX_DEPTH {
                    return Err(Error::TooLarge);
                }
                let attrs = e
                    .attributes()
                    .with_checks(false)
                    .flatten()
                    .map(|a| (a.key.as_ref().to_string(), unescape(&a.value)))
                    .collect();
                let node = Node { name: e.name().as_ref().to_string(), attrs, ..Default::default() };
                if matches!(ev, Event::Empty(_)) {
                    if let Some(p) = stack.last_mut() {
                        p.children.push(node);
                    }
                } else {
                    stack.push(node);
                }
            }
            Event::End(_) => close(&mut stack),
            Event::Text(t) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(t.as_ref());
                }
            }
            Event::CData(t) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(t.as_ref());
                }
            }
            Event::GeneralRef(r) => {
                if let (Some(top), Some(c)) = (stack.last_mut(), entity(r.as_ref())) {
                    top.text.push(c);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    while stack.len() > 1 {
        close(&mut stack);
    }
    stack.pop().ok_or(Error::Xml("empty document".into()))
}

fn close(stack: &mut Vec<Node>) {
    if stack.len() > 1 {
        if let Some(n) = stack.pop() {
            if let Some(p) = stack.last_mut() {
                p.children.push(n);
            }
        }
    }
}
