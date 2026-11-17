//! How big each picture of a PDF is on its pages, for Compress: from the matrices in the content
//! that draws it, or from its page where that can't be read.

use std::collections::{HashMap, HashSet};

use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream};

use crate::model::pdf_place::{Matrix, Op, draws};

/// More `Do`s than this in a file and the rest are taken as page-sized, which only keeps more.
const MAX_DRAWS: usize = 200_000;
/// Content longer than this is not read; its pictures are taken as page-sized.
const MAX_CONTENT: usize = 32 << 20;
const MAX_FORM_DEPTH: usize = 8;

/// For every picture, how big it is shown (width, height in points) at its largest. A picture
/// whose drawing can't be found is as big as the largest page it is on, which can only overstate
/// it, so it is never shrunk below the limit where it really appears.
pub fn picture_sizes(doc: &Document) -> HashMap<ObjectId, (f64, f64)> {
    let mut sizes = page_sizes(doc);
    let (mut budget, mut shown) = (MAX_DRAWS, HashMap::new());
    for page in doc.page_iter() {
        let Ok((own, inherited)) = doc.get_page_resources(page) else {
            continue;
        };
        let scope: Vec<&Dictionary> = own
            .into_iter()
            .chain(
                inherited
                    .iter()
                    .filter_map(|id| doc.get_dictionary(*id).ok()),
            )
            .collect();
        let content = doc.get_page_content(page);
        let mut walk = Walk {
            doc,
            budget: &mut budget,
            shown: &mut shown,
        };
        walk.content(&content, &scope, Matrix::IDENTITY, 0);
    }
    sizes.extend(shown);
    sizes
}

struct Walk<'a, 'd> {
    doc: &'d Document,
    budget: &'a mut usize,
    shown: &'a mut HashMap<ObjectId, (f64, f64)>,
}

impl Walk<'_, '_> {
    fn content(&mut self, bytes: &[u8], scope: &[&Dictionary], start: Matrix, depth: usize) {
        if bytes.len() > MAX_CONTENT {
            return;
        }
        let Ok(content) = Content::decode(bytes) else {
            return;
        };
        let ops: Vec<Op> = content.operations.iter().filter_map(op).collect();
        for (name, at) in draws(&ops, start) {
            if *self.budget == 0 {
                return;
            }
            *self.budget -= 1;
            let Some(id) = xobject(self.doc, scope, name) else {
                continue;
            };
            let Ok(stream) = self.doc.get_object(id).and_then(Object::as_stream) else {
                continue;
            };
            match subtype(stream) {
                Some(b"Image") => {
                    if let Some(size) = at.shown() {
                        let entry = self.shown.entry(id).or_insert(size);
                        if size.0 * size.1 > entry.0 * entry.1 {
                            *entry = size;
                        }
                    }
                }
                Some(b"Form") if depth < MAX_FORM_DEPTH => self.form(stream, scope, at, depth),
                _ => {}
            }
        }
    }

    /// A form is drawn in its own space, and may bring its own resources.
    fn form(&mut self, form: &Stream, outer: &[&Dictionary], at: Matrix, depth: usize) {
        let own = form
            .dict
            .get(b"Resources")
            .and_then(|r| self.doc.dereference(r))
            .and_then(|(_, r)| r.as_dict());
        let scope: Vec<&Dictionary> = own.into_iter().chain(outer.iter().copied()).collect();
        let m = form
            .dict
            .get(b"Matrix")
            .ok()
            .and_then(|m| matrix(m.as_array().ok()?))
            .unwrap_or(Matrix::IDENTITY);
        if let Ok(bytes) = form.decompressed_content() {
            self.content(&bytes, &scope, m.then(at), depth + 1);
        }
    }
}

fn op(o: &Operation) -> Option<Op> {
    match o.operator.as_str() {
        "q" => Some(Op::Save),
        "Q" => Some(Op::Restore),
        "cm" => matrix(&o.operands).map(Op::Concat),
        "Do" => Some(Op::Draw(o.operands.first()?.as_name().ok()?.to_vec())),
        _ => None,
    }
}

fn matrix(numbers: &[Object]) -> Option<Matrix> {
    let v: Vec<f64> = numbers
        .iter()
        .filter_map(|n| n.as_float().ok().map(f64::from))
        .collect();
    Some(Matrix(v.try_into().ok()?))
}

fn subtype(s: &Stream) -> Option<&[u8]> {
    s.dict.get(b"Subtype").and_then(Object::as_name).ok()
}

fn xobject(doc: &Document, scope: &[&Dictionary], name: &[u8]) -> Option<ObjectId> {
    scope.iter().find_map(|res| {
        let (_, xobjects) = doc.dereference(res.get(b"XObject").ok()?).ok()?;
        xobjects.as_dict().ok()?.get(name).ok()?.as_reference().ok()
    })
}

/// For every picture a page draws (directly or through a form), the largest page it is on.
fn page_sizes(doc: &Document) -> HashMap<ObjectId, (f64, f64)> {
    let mut sizes: HashMap<ObjectId, (f64, f64)> = HashMap::new();
    for page in doc.page_iter() {
        let size = media_box(doc, page).unwrap_or((612.0, 792.0));
        let Ok((resources, inherited)) = doc.get_page_resources(page) else {
            continue;
        };
        let mut stack: Vec<&Dictionary> = resources.into_iter().collect();
        stack.extend(
            inherited
                .iter()
                .filter_map(|id| doc.get_dictionary(*id).ok()),
        );
        let mut seen = HashSet::new();
        while let Some(res) = stack.pop() {
            let Ok(xobjects) = res
                .get(b"XObject")
                .and_then(|x| doc.dereference(x))
                .and_then(|(_, x)| x.as_dict())
            else {
                continue;
            };
            for (_, x) in xobjects.iter() {
                let Ok(id) = x.as_reference() else { continue };
                if !seen.insert(id) || seen.len() > 10_000 {
                    continue;
                }
                let entry = sizes.entry(id).or_insert(size);
                if size.0 * size.1 > entry.0 * entry.1 {
                    *entry = size;
                }
                if let Ok(form) = doc.get_object(id).and_then(Object::as_stream)
                    && let Ok(inner) = form
                        .dict
                        .get(b"Resources")
                        .and_then(|r| doc.dereference(r))
                        .and_then(|(_, r)| r.as_dict())
                {
                    stack.push(inner);
                }
            }
        }
    }
    sizes
}

pub fn media_box(doc: &Document, page: ObjectId) -> Option<(f64, f64)> {
    let mut node = doc.get_dictionary(page).ok()?;
    for _ in 0..64 {
        if let Ok(b) = node
            .get(b"MediaBox")
            .and_then(|b| doc.dereference(b))
            .and_then(|(_, b)| b.as_array())
        {
            let v: Vec<f64> = b
                .iter()
                .filter_map(|n| n.as_float().ok().map(f64::from))
                .collect();
            return (v.len() == 4).then(|| ((v[2] - v[0]).abs(), (v[3] - v[1]).abs()));
        }
        node = doc
            .get_dictionary(node.get(b"Parent").ok()?.as_reference().ok()?)
            .ok()?;
    }
    None
}
