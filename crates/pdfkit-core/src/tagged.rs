//! Tagged-PDF logical structure (PRD §4.1 "tagged structure").
//!
//! Reads the catalog `/StructTreeRoot` — the author-provided logical structure
//! tree — into a [`StructNode`] tree: standard element types (heading levels,
//! `Table`/`TR`/`TH`/`TD`, `L`/`LI`, `Figure`, …) resolved through the
//! `/RoleMap`, the text of each element (collected from its marked content via
//! MCIDs), figure alt-text, the page each element sits on, and reading order
//! (the tree order *is* the logical reading order). When a document is tagged
//! this is ground truth — far better than geometry heuristics. Returns `None`
//! when the document isn't tagged.

use std::collections::HashMap;
use std::collections::HashSet;

use lopdf::content::Content;
use lopdf::{Dictionary, Document as LoDoc, Encoding, Object, ObjectId};

/// A node in the tagged-PDF structure tree.
#[derive(Debug, Clone, PartialEq, Eq)]
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

    // MCID -> text per page, computed once (MCIDs are page-local).
    // TODO(design): a structure element whose /Pg points outside the /Pages tree
    // (malformed) isn't keyed here, so its text comes back empty — best-effort.
    let mcid_text: HashMap<ObjectId, HashMap<u32, String>> = page_ids
        .iter()
        .map(|&pid| (pid, mcid_text(doc, pid)))
        .collect();

    let ctx = Ctx {
        doc,
        page_ids,
        role_map,
        mcid_text: &mcid_text,
    };
    let mut visited = HashSet::new();
    let children = ctx.kids_of(root, None, &mut visited, 0);
    Some(StructNode {
        tag: "Root".to_string(),
        raw_tag: "Root".to_string(),
        text: String::new(),
        alt: None,
        page: None,
        children,
    })
}

/// Shared, read-only state threaded through the recursive build.
struct Ctx<'a> {
    doc: &'a LoDoc,
    page_ids: &'a [ObjectId],
    role_map: Option<&'a Dictionary>,
    mcid_text: &'a HashMap<ObjectId, HashMap<u32, String>>,
}

impl Ctx<'_> {
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
            // A structure tree is a tree (each element has one /P), so a
            // global visited set both stops cycles and bounds work to O(nodes).
            // On malformed input that shares a child across parents we expand it
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
            .map(decode_text_string);
        let page_id = elem
            .get(b"Pg")
            .ok()
            .and_then(|o| o.as_reference().ok())
            .or(inherited_pg);

        let text = self.collect_text(elem, page_id);
        let children = self.kids_of(elem, page_id, visited, depth);

        StructNode {
            tag,
            raw_tag,
            text,
            alt,
            page: page_id.and_then(|id| self.page_number_of(id)),
            children,
        }
    }

    /// Collect the element's own marked-content text: integer MCID kids (on the
    /// element's page) and MCR kids (on their own `/Pg`, else the element's).
    fn collect_text(&self, elem: &Dictionary, page_id: Option<ObjectId>) -> String {
        let mut text = String::new();
        for kid in kids_array(elem) {
            match kid {
                Object::Integer(mcid) => {
                    if let (Some(pg), Ok(mcid)) = (page_id, u32::try_from(*mcid)) {
                        if let Some(s) = self.mcid_text.get(&pg).and_then(|m| m.get(&mcid)) {
                            text.push_str(s);
                        }
                    }
                }
                Object::Reference(id) => {
                    if let Ok(dict) = self.doc.get_dictionary(*id) {
                        self.push_mcr_text(dict, page_id, &mut text);
                    }
                }
                Object::Dictionary(dict) => self.push_mcr_text(dict, page_id, &mut text),
                _ => {}
            }
        }
        text
    }

    /// If `dict` is a marked-content reference (`/Type /MCR`), append its text.
    fn push_mcr_text(&self, dict: &Dictionary, fallback_pg: Option<ObjectId>, text: &mut String) {
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
            if let Some(s) = self.mcid_text.get(&pg).and_then(|m| m.get(&mcid)) {
                text.push_str(s);
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

/// Walk a page's content once, collecting MCID -> concatenated text. Tracks
/// nested marked content (`BDC`/`BMC`/`EMC`) and the active font encoding so the
/// text decodes the same way as the main extractor.
fn mcid_text(doc: &LoDoc, page_id: ObjectId) -> HashMap<u32, String> {
    let mut out: HashMap<u32, String> = HashMap::new();
    let Ok(content) = doc.get_page_content(page_id) else {
        return out;
    };
    let Ok(parsed) = Content::decode(&content) else {
        return out;
    };
    let encodings = crate::textrun::font_encodings(doc, page_id);
    let mut stack: Vec<Option<u32>> = Vec::new();
    let mut encoding: Option<&Encoding> = None;

    let append = |stack: &[Option<u32>], out: &mut HashMap<u32, String>, s: &str| {
        // Attribute text to the nearest enclosing MCID.
        if let Some(mcid) = stack.iter().rev().copied().flatten().next() {
            out.entry(mcid).or_default().push_str(s);
        }
    };

    for op in &parsed.operations {
        match op.operator.as_str() {
            "BDC" => stack.push(bdc_mcid(doc, page_id, &op.operands)),
            "BMC" => stack.push(None),
            "EMC" => {
                stack.pop();
            }
            "Tf" => {
                encoding = op
                    .operands
                    .first()
                    .and_then(|o| o.as_name().ok())
                    .and_then(|n| encodings.get(n));
            }
            "Tj" | "'" => {
                if let Some(bytes) = op.operands.last().and_then(|o| o.as_str().ok()) {
                    append(&stack, &mut out, &crate::textrun::decode(encoding, bytes));
                }
            }
            "\"" => {
                if let Some(bytes) = op.operands.get(2).and_then(|o| o.as_str().ok()) {
                    append(&stack, &mut out, &crate::textrun::decode(encoding, bytes));
                }
            }
            "TJ" => {
                if let Some(Object::Array(items)) = op.operands.first() {
                    for item in items {
                        if let Object::String(bytes, _) = item {
                            append(&stack, &mut out, &crate::textrun::decode(encoding, bytes));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// The MCID of a `BDC` operator, from an inline properties dict (`/MCID n`) or a
/// name referencing the page `/Resources` `/Properties`.
fn bdc_mcid(doc: &LoDoc, page_id: ObjectId, operands: &[Object]) -> Option<u32> {
    let props = operands.get(1)?;
    let dict = match props {
        Object::Dictionary(d) => d,
        Object::Name(name) => {
            let page = doc.get_dictionary(page_id).ok()?;
            let resources = page
                .get(b"Resources")
                .ok()
                .and_then(|o| deref_dict(doc, o))?;
            let properties = resources
                .get(b"Properties")
                .ok()
                .and_then(|o| deref_dict(doc, o))?;
            properties.get(name).ok().and_then(|o| deref_dict(doc, o))?
        }
        _ => return None,
    };
    dict.get(b"MCID")
        .ok()
        .and_then(|o| o.as_i64().ok())
        .and_then(|n| u32::try_from(n).ok())
}

/// Dereference one level to a dictionary.
fn deref_dict<'a>(doc: &'a LoDoc, obj: &'a Object) -> Option<&'a Dictionary> {
    match obj.as_reference() {
        Ok(id) => doc.get_dictionary(id).ok(),
        Err(_) => obj.as_dict().ok(),
    }
}

/// Decode a PDF text string (UTF-16BE with BOM, else Latin-1) for `/Alt` etc.
fn decode_text_string(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        let pairs = bytes[2..].chunks_exact(2);
        let trailing = !pairs.remainder().is_empty();
        let mut units: Vec<u16> = pairs.map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        if trailing {
            units.push(0xFFFD);
        }
        String::from_utf16_lossy(&units)
    } else {
        bytes.iter().map(|&b| b as char).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx<'a>(
        doc: &'a LoDoc,
        role_map: &'a Dictionary,
        empty: &'a HashMap<ObjectId, HashMap<u32, String>>,
    ) -> Ctx<'a> {
        Ctx {
            doc,
            page_ids: &[],
            role_map: Some(role_map),
            mcid_text: empty,
        }
    }

    #[test]
    fn role_map_resolves_direct_chain_passthrough_and_cycle() {
        let doc = LoDoc::new();
        let empty = HashMap::new();
        let mut rm = Dictionary::new();
        rm.set("Heading1", Object::Name(b"H1".to_vec())); // custom -> standard
        rm.set("A", Object::Name(b"B".to_vec()));
        rm.set("B", Object::Name(b"C".to_vec())); // chain A -> B -> C
        rm.set("X", Object::Name(b"Y".to_vec()));
        rm.set("Y", Object::Name(b"X".to_vec())); // cycle X <-> Y
        let ctx = ctx(&doc, &rm, &empty);

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
        let empty = HashMap::new();
        let mut rm = Dictionary::new();
        rm.set("Sub", Object::Reference(id));
        let ctx = ctx(&doc, &rm, &empty);
        assert_eq!(ctx.resolve_role(b"Sub"), "H2");
    }
}
