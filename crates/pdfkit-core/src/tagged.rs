//! Tagged-PDF logical structure (PRD §4.1 "tagged structure").
//!
//! Reads the catalog `/StructTreeRoot` — the author-provided logical structure
//! tree — into a [`StructNode`] tree: standard element types (heading levels,
//! `Table`/`TR`/`TH`/`TD`, `L`/`LI`, `Figure`, …) resolved through the
//! `/RoleMap`, each element's text and bounding box (collected from its marked
//! content via MCIDs), figure alt-text, the page it sits on, and reading order
//! (the tree order *is* the logical reading order). When a document is tagged
//! this is ground truth — far better than geometry heuristics. Returns `None`
//! when the document isn't tagged.

use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::HashSet;

use lopdf::{Dictionary, Document as LoDoc, Object, ObjectId};

/// A node in the tagged-PDF structure tree.
#[derive(Debug, Clone, PartialEq)]
pub struct StructNode {
    /// Standard structure type after `/RoleMap` resolution (e.g. `"H1"`, `"P"`,
    /// `"Table"`, `"Figure"`). Equals [`StructNode::raw_tag`] when no mapping
    /// applied. The synthetic tree root is `"Root"`.
    pub tag: String,
    /// The element's own `/S` type, before `/RoleMap` resolution.
    pub raw_tag: String,
    /// Text from this element's *own* marked content (its MCID/MCR kids). Child
    /// elements carry their own text; aggregate by recursing `children`.
    pub text: String,
    /// Alternate text (`/Alt`), e.g. a `Figure`'s description.
    pub alt: Option<String>,
    /// One-based page the element's content is on, if resolvable.
    pub page: Option<usize>,
    /// Bounding box `[x0, y0, x1, y1]` in points — the union of this element's
    /// marked-content and its descendants', when any of it is positioned.
    pub bbox: Option<[f32; 4]>,
    /// Child structure elements, in reading order.
    pub children: Vec<StructNode>,
}

const MAX_DEPTH: usize = 64;
const MAX_ROLE_DEPTH: usize = 16;

/// Build the structure tree for a tagged document, or `None` if it isn't tagged
/// (`/MarkInfo` `/Marked true` + a `/StructTreeRoot`). `page_ids` maps page
/// object ids to one-based numbers (index 0 == page 1).
pub(crate) fn structure_tree(doc: &LoDoc, page_ids: &[ObjectId]) -> Option<StructNode> {
    let catalog = doc.catalog().ok()?;
    let marked = catalog
        .get(b"MarkInfo")
        .ok()
        .and_then(|o| deref_dict(doc, o))
        .and_then(|d| d.get(b"Marked").ok())
        .and_then(|o| o.as_bool().ok())
        .unwrap_or(false);
    if !marked {
        return None;
    }
    let root_id = catalog.get(b"StructTreeRoot").ok()?.as_reference().ok()?;
    let root = doc.get_dictionary(root_id).ok()?;
    let role_map = root.get(b"RoleMap").ok().and_then(|o| deref_dict(doc, o));

    let ctx = Ctx {
        doc,
        page_ids,
        role_map,
        // MCID -> (text, bbox) per page, computed lazily on first reference so
        // any /Pg (even one outside the /Pages tree) is handled and pages with
        // no structure aren't walked.
        cache: RefCell::new(HashMap::new()),
    };
    let mut visited = HashSet::new();
    let children = ctx.kids_of(root, None, &mut visited, 0);
    Some(StructNode {
        tag: "Root".to_string(),
        raw_tag: "Root".to_string(),
        text: String::new(),
        alt: None,
        page: None,
        bbox: None,
        children,
    })
}

/// One marked-content sequence's accumulated text and bounding box.
#[derive(Clone, Default)]
struct McidContent {
    text: String,
    bbox: Option<[f32; 4]>,
}

/// Shared state threaded through the recursive build.
struct Ctx<'a> {
    doc: &'a LoDoc,
    page_ids: &'a [ObjectId],
    role_map: Option<&'a Dictionary>,
    cache: RefCell<HashMap<ObjectId, HashMap<u32, McidContent>>>,
}

impl Ctx<'_> {
    /// Text + bbox for a marked-content id on a page, computing that page's
    /// MCID map on first use.
    fn mcid(&self, page_id: ObjectId, mcid: u32) -> Option<(String, Option<[f32; 4]>)> {
        if !self.cache.borrow().contains_key(&page_id) {
            let content = page_mcid_content(self.doc, page_id);
            self.cache.borrow_mut().insert(page_id, content);
        }
        self.cache
            .borrow()
            .get(&page_id)?
            .get(&mcid)
            .map(|c| (c.text.clone(), c.bbox))
    }

    /// The child structure elements of `parent` (skipping marked-content kids,
    /// which belong to the element's own text). `inherited_pg` is the nearest
    /// ancestor `/Pg`, used when an element omits its own.
    fn kids_of(
        &self,
        parent: &Dictionary,
        inherited_pg: Option<ObjectId>,
        visited: &mut HashSet<ObjectId>,
        depth: usize,
    ) -> Vec<StructNode> {
        if depth > MAX_DEPTH {
            return Vec::new();
        }
        let mut out = Vec::new();
        for kid in kids_array(parent) {
            // Only references to *structure elements* become child nodes.
            let Ok(id) = kid.as_reference() else {
                continue;
            };
            let Ok(dict) = self.doc.get_dictionary(id) else {
                continue;
            };
            if is_marked_content_kid(dict) {
                continue; // MCR/OBJR belong to the parent's content, not children
            }
            // A structure tree is a tree (each element has one /P), so a global
            // visited set both stops cycles and bounds work to O(nodes). On
            // malformed input that shares a child across parents we expand it
            // under the first parent only — acceptable, and avoids the
            // exponential blow-up a path-scoped guard would risk.
            if !visited.insert(id) {
                continue;
            }
            out.push(self.build_node(dict, inherited_pg, visited, depth + 1));
        }
        out
    }

    /// Build one [`StructNode`] from a structure-element dict.
    fn build_node(
        &self,
        elem: &Dictionary,
        inherited_pg: Option<ObjectId>,
        visited: &mut HashSet<ObjectId>,
        depth: usize,
    ) -> StructNode {
        let raw_tag = elem
            .get(b"S")
            .ok()
            .and_then(|o| o.as_name().ok())
            .map(|n| String::from_utf8_lossy(n).into_owned())
            .unwrap_or_else(|| "Unknown".to_string());
        let tag = self.resolve_role(raw_tag.as_bytes());
        let alt = elem
            .get(b"Alt")
            .ok()
            .and_then(|o| o.as_str().ok())
            .map(crate::pdfstr::decode_pdf_text);
        let page_id = elem
            .get(b"Pg")
            .ok()
            .and_then(|o| o.as_reference().ok())
            .or(inherited_pg);

        let (text, own_bbox) = self.collect_text(elem, page_id);
        let children = self.kids_of(elem, page_id, visited, depth);
        // The element's box spans its own content plus its descendants'.
        let bbox = children
            .iter()
            .fold(own_bbox, |acc, c| union_opt(acc, c.bbox));

        StructNode {
            tag,
            raw_tag,
            text,
            alt,
            page: page_id.and_then(|id| self.page_number_of(id)),
            bbox,
            children,
        }
    }

    /// The element's own marked-content text and bbox: integer MCID kids (on the
    /// element's page) and MCR kids (on their own `/Pg`, else the element's).
    fn collect_text(
        &self,
        elem: &Dictionary,
        page_id: Option<ObjectId>,
    ) -> (String, Option<[f32; 4]>) {
        let mut text = String::new();
        let mut bbox = None;
        for kid in kids_array(elem) {
            match kid {
                Object::Integer(mcid) => {
                    if let (Some(pg), Ok(mcid)) = (page_id, u32::try_from(*mcid)) {
                        if let Some((t, b)) = self.mcid(pg, mcid) {
                            text.push_str(&t);
                            bbox = union_opt(bbox, b);
                        }
                    }
                }
                Object::Reference(id) => {
                    if let Ok(dict) = self.doc.get_dictionary(*id) {
                        self.collect_mcr(dict, page_id, &mut text, &mut bbox);
                    }
                }
                Object::Dictionary(dict) => self.collect_mcr(dict, page_id, &mut text, &mut bbox),
                _ => {}
            }
        }
        (text, bbox)
    }

    /// If `dict` is a marked-content reference (`/Type /MCR`), append its text
    /// and grow the bounding box.
    fn collect_mcr(
        &self,
        dict: &Dictionary,
        fallback_pg: Option<ObjectId>,
        text: &mut String,
        bbox: &mut Option<[f32; 4]>,
    ) {
        let is_mcr = dict
            .get(b"Type")
            .ok()
            .and_then(|o| o.as_name().ok())
            .map(|n| n == b"MCR")
            .unwrap_or(false);
        if !is_mcr {
            return;
        }
        let Some(mcid) = dict
            .get(b"MCID")
            .ok()
            .and_then(|o| o.as_i64().ok())
            .and_then(|n| u32::try_from(n).ok())
        else {
            return;
        };
        let pg = dict
            .get(b"Pg")
            .ok()
            .and_then(|o| o.as_reference().ok())
            .or(fallback_pg);
        if let Some(pg) = pg {
            if let Some((t, b)) = self.mcid(pg, mcid) {
                text.push_str(&t);
                *bbox = union_opt(*bbox, b);
            }
        }
    }

    /// Resolve a structure type through the `/RoleMap`, bounded against cycles.
    fn resolve_role(&self, raw: &[u8]) -> String {
        let mut name = raw.to_vec();
        let mut seen = HashSet::new();
        for _ in 0..MAX_ROLE_DEPTH {
            if !seen.insert(name.clone()) {
                break;
            }
            let Some(mapped) = self
                .role_map
                .and_then(|rm| rm.get(name.as_slice()).ok())
                // Tolerate a (non-conformant) indirect name value.
                .and_then(|o| match o.as_reference() {
                    Ok(id) => self.doc.get_object(id).ok(),
                    Err(_) => Some(o),
                })
                .and_then(|o| o.as_name().ok())
            else {
                break;
            };
            name = mapped.to_vec();
        }
        String::from_utf8_lossy(&name).into_owned()
    }

    fn page_number_of(&self, id: ObjectId) -> Option<usize> {
        self.page_ids.iter().position(|&p| p == id).map(|i| i + 1)
    }
}

/// MCID -> (text, bbox) for a page, derived from the positioned text runs (the
/// same extractor the rest of the crate uses, so decoding/positioning match).
fn page_mcid_content(doc: &LoDoc, page_id: ObjectId) -> HashMap<u32, McidContent> {
    let mut out: HashMap<u32, McidContent> = HashMap::new();
    for run in crate::textrun::page_text_runs(doc, page_id) {
        let Some(mcid) = run.mcid else {
            continue;
        };
        let entry = out.entry(mcid).or_default();
        entry.text.push_str(&run.text);
        entry.bbox = union_opt(entry.bbox, Some(run.bbox));
    }
    out
}

/// Union of two bounding boxes.
fn union(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[2].max(b[2]),
        a[3].max(b[3]),
    ]
}

/// Union of two optional bounding boxes (`None` is the identity).
fn union_opt(a: Option<[f32; 4]>, b: Option<[f32; 4]>) -> Option<[f32; 4]> {
    match (a, b) {
        (Some(x), Some(y)) => Some(union(x, y)),
        (Some(x), None) => Some(x),
        (None, y) => y,
    }
}

/// `/K` normalized to a slice of kid objects (single kid -> one-element slice).
fn kids_array(dict: &Dictionary) -> Vec<&Object> {
    match dict.get(b"K") {
        Ok(Object::Array(arr)) => arr.iter().collect(),
        Ok(other) => vec![other],
        Err(_) => Vec::new(),
    }
}

/// Whether a kid dict is a marked-content (`/MCR`) or object (`/OBJR`) reference
/// rather than a child structure element.
fn is_marked_content_kid(dict: &Dictionary) -> bool {
    dict.get(b"Type")
        .ok()
        .and_then(|o| o.as_name().ok())
        .map(|n| n == b"MCR" || n == b"OBJR")
        .unwrap_or(false)
}

/// Dereference one level to a dictionary.
fn deref_dict<'a>(doc: &'a LoDoc, obj: &'a Object) -> Option<&'a Dictionary> {
    match obj.as_reference() {
        Ok(id) => doc.get_dictionary(id).ok(),
        Err(_) => obj.as_dict().ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx<'a>(doc: &'a LoDoc, role_map: &'a Dictionary) -> Ctx<'a> {
        Ctx {
            doc,
            page_ids: &[],
            role_map: Some(role_map),
            cache: RefCell::new(HashMap::new()),
        }
    }

    #[test]
    fn role_map_resolves_direct_chain_passthrough_and_cycle() {
        let doc = LoDoc::new();
        let mut rm = Dictionary::new();
        rm.set("Heading1", Object::Name(b"H1".to_vec())); // custom -> standard
        rm.set("A", Object::Name(b"B".to_vec()));
        rm.set("B", Object::Name(b"C".to_vec())); // chain A -> B -> C
        rm.set("X", Object::Name(b"Y".to_vec()));
        rm.set("Y", Object::Name(b"X".to_vec())); // cycle X <-> Y
        let ctx = ctx(&doc, &rm);

        assert_eq!(ctx.resolve_role(b"Heading1"), "H1");
        assert_eq!(ctx.resolve_role(b"A"), "C");
        assert_eq!(ctx.resolve_role(b"P"), "P"); // unmapped -> passthrough
        let cyclic = ctx.resolve_role(b"X"); // must terminate, not hang
        assert!(cyclic == "X" || cyclic == "Y");
    }

    #[test]
    fn role_map_dereferences_indirect_name() {
        let mut doc = LoDoc::new();
        let id = doc.add_object(Object::Name(b"H2".to_vec()));
        let mut rm = Dictionary::new();
        rm.set("Sub", Object::Reference(id));
        let ctx = ctx(&doc, &rm);
        assert_eq!(ctx.resolve_role(b"Sub"), "H2");
    }
}
