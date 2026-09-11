//! Walks the document and converts every place that can hold color: page contents, form
//! XObjects, images, patterns, shadings, Type 3 glyphs, annotations and form fields.

use std::collections::{HashMap, HashSet};

use lopdf::{Dictionary, Document, Object, ObjectId, StringFormat};

use crate::colorspace::ColorSpace;
use crate::content::{self, Env};
use crate::error::{Problem, SumiError};
use crate::objects::{decode_stream, flate_stream, get_int, get_name, number, set_flate_content};
use crate::options::Settings;
use crate::report::Report;

/// A dictionary inside an object, reached through direct (non-reference) dictionary keys.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DictLoc {
    id: ObjectId,
    path: Vec<Vec<u8>>,
}

impl DictLoc {
    fn object(id: ObjectId) -> Self {
        DictLoc {
            id,
            path: Vec::new(),
        }
    }
}

fn dict_at<'a>(doc: &'a Document, loc: &DictLoc) -> Option<&'a Dictionary> {
    let mut dict = match doc.get_object(loc.id).ok()? {
        Object::Dictionary(d) => d,
        Object::Stream(s) => &s.dict,
        _ => return None,
    };
    for key in &loc.path {
        dict = match dict.get(key).ok()? {
            Object::Dictionary(d) => d,
            _ => return None,
        };
    }
    Some(dict)
}

fn dict_at_mut<'a>(doc: &'a mut Document, loc: &DictLoc) -> Option<&'a mut Dictionary> {
    let mut dict = match doc.get_object_mut(loc.id).ok()? {
        Object::Dictionary(d) => d,
        Object::Stream(s) => &mut s.dict,
        _ => return None,
    };
    for key in &loc.path {
        dict = match dict.get_mut(key).ok()? {
            Object::Dictionary(d) => d,
            _ => return None,
        };
    }
    Some(dict)
}

/// The dictionary stored under `key`, following one reference.
fn child(doc: &Document, loc: &DictLoc, key: &[u8]) -> Option<DictLoc> {
    let found = match dict_at(doc, loc)?.get(key).ok()? {
        Object::Reference(id) => DictLoc::object(*id),
        Object::Dictionary(_) => {
            let mut path = loc.path.clone();
            path.push(key.to_vec());
            DictLoc { id: loc.id, path }
        }
        _ => return None,
    };
    dict_at(doc, &found).map(|_| found)
}

/// All dictionaries and streams stored as values of the dictionary at `loc`.
fn children(doc: &Document, loc: &DictLoc) -> Vec<DictLoc> {
    let Some(dict) = dict_at(doc, loc) else {
        return Vec::new();
    };
    let keys: Vec<Vec<u8>> = dict.iter().map(|(k, _)| k.clone()).collect();
    keys.iter().filter_map(|k| child(doc, loc, k)).collect()
}

fn is_stream(doc: &Document, loc: &DictLoc) -> bool {
    loc.path.is_empty() && matches!(doc.get_object(loc.id), Ok(Object::Stream(_)))
}

pub(crate) struct Converter<'s> {
    pub doc: Document,
    pub report: Report,
    settings: &'s Settings,
    /// Content streams and images already converted.
    done_streams: HashSet<ObjectId>,
    /// Resource dictionaries, patterns, shadings and annotations already visited.
    done_dicts: HashSet<DictLoc>,
    /// Rewritten page contents, keyed by the original streams and resources.
    page_contents: HashMap<(Vec<ObjectId>, Option<DictLoc>), Option<ObjectId>>,
    form_resources: Option<DictLoc>,
}

impl<'s> Converter<'s> {
    pub(crate) fn new(doc: Document, settings: &'s Settings) -> Self {
        Converter {
            doc,
            report: Report::default(),
            settings,
            done_streams: HashSet::new(),
            done_dicts: HashSet::new(),
            page_contents: HashMap::new(),
            form_resources: None,
        }
    }

    pub(crate) fn run(&mut self) -> Result<(), SumiError> {
        let pages: Vec<ObjectId> = self.doc.get_pages().into_values().collect();
        self.report.pages = pages.len();
        let acroform = self
            .doc
            .trailer
            .get(b"Root")
            .ok()
            .and_then(|r| r.as_reference().ok())
            .and_then(|root| child(&self.doc, &DictLoc::object(root), b"AcroForm"));
        self.form_resources = acroform.as_ref().and_then(|af| child(&self.doc, af, b"DR"));

        for page in pages {
            self.page(page)?;
        }
        if let Some(acroform) = acroform {
            self.acroform(&acroform)?;
        }
        Ok(())
    }

    fn outcome<T>(
        &mut self,
        what: &str,
        result: Result<T, Problem>,
    ) -> Result<Option<T>, SumiError> {
        match result {
            Ok(v) => Ok(Some(v)),
            Err(Problem::Limit(msg)) => Err(SumiError::LimitExceeded(msg)),
            Err(Problem::Unsupported(msg) | Problem::Invalid(msg)) => {
                self.report.warn(format!("{what} not converted: {msg}"));
                Ok(None)
            }
        }
    }

    fn page(&mut self, page_id: ObjectId) -> Result<(), SumiError> {
        let page = DictLoc::object(page_id);
        let resources = self.inherited_resources(page_id);

        let content_ids = self.doc.get_page_contents(page_id);
        if !content_ids.is_empty() {
            let key = (content_ids.clone(), resources.clone());
            let new_id = match self.page_contents.get(&key) {
                Some(id) => *id,
                None => {
                    let id = self.page_content(&page, &content_ids, resources.as_ref())?;
                    self.page_contents.insert(key, id);
                    id
                }
            };
            if let Some(new_id) = new_id
                && let Some(dict) = dict_at_mut(&mut self.doc, &page)
            {
                dict.set("Contents", Object::Reference(new_id));
            }
        }

        if let Some(resources) = &resources {
            self.resources(resources, 0)?;
        }
        self.group(&page);

        let annots: Vec<ObjectId> = dict_at(&self.doc, &page)
            .and_then(|d| d.get(b"Annots").ok())
            .map(|a| crate::objects::deref(&self.doc, a))
            .and_then(|a| a.as_array().ok())
            .map(|items| items.iter().filter_map(|o| o.as_reference().ok()).collect())
            .unwrap_or_default();
        for annot in annots {
            self.annotation(&DictLoc::object(annot))?;
        }
        Ok(())
    }

    /// `/Resources` is inherited from the nearest ancestor in the page tree.
    fn inherited_resources(&self, page_id: ObjectId) -> Option<DictLoc> {
        let mut node = page_id;
        let mut seen = HashSet::new();
        while seen.insert(node) && seen.len() < 64 {
            let loc = DictLoc::object(node);
            if let Some(found) = child(&self.doc, &loc, b"Resources") {
                return Some(found);
            }
            node = dict_at(&self.doc, &loc)?
                .get(b"Parent")
                .ok()?
                .as_reference()
                .ok()?;
        }
        None
    }

    fn page_content(
        &mut self,
        page: &DictLoc,
        ids: &[ObjectId],
        resources: Option<&DictLoc>,
    ) -> Result<Option<ObjectId>, SumiError> {
        let limit = self.settings.limits.max_stream_bytes;
        let mut data = Vec::new();
        for id in ids {
            let Ok(Object::Stream(stream)) = self.doc.get_object(*id) else {
                continue;
            };
            let decoded = decode_stream(&self.doc, stream, limit);
            let Some(decoded) = self.outcome("page content", decoded)? else {
                return Ok(None);
            };
            if data.len() + decoded.len() > limit {
                return Err(SumiError::LimitExceeded(format!(
                    "page content is larger than {limit} bytes"
                )));
            }
            data.extend_from_slice(&decoded);
            data.push(b'\n');
        }

        let Some((output, name)) = self.rewrite(&data, resources, false, "page content")? else {
            return Ok(None);
        };
        if output.uses_pattern_gray {
            self.add_pattern_gray(resources, page, &name);
        }
        if !output.changed {
            return Ok(None);
        }
        self.report.content_streams += 1;
        Ok(Some(
            self.doc
                .add_object(flate_stream(Dictionary::new(), &output.data)),
        ))
    }

    fn rewrite(
        &mut self,
        data: &[u8],
        resources: Option<&DictLoc>,
        inherited_state: bool,
        what: &str,
    ) -> Result<Option<(content::Output, Vec<u8>)>, SumiError> {
        let resource_dict = resources.and_then(|loc| dict_at(&self.doc, loc));
        let existing =
            resource_dict.and_then(|r| crate::objects::get_dict(&self.doc, r, b"ColorSpace"));
        let mut name = b"SumiPatternGray".to_vec();
        let mut n = 1;
        while existing.is_some_and(|cs| cs.has(&name)) {
            name = format!("SumiPatternGray{n}").into_bytes();
            n += 1;
        }
        let env = Env {
            doc: &self.doc,
            resources: resource_dict,
            settings: self.settings,
            pattern_gray_name: &name,
            inherited_state,
        };
        let result = content::rewrite(data, &env, &mut self.report);
        Ok(self.outcome(what, result)?.map(|output| (output, name)))
    }

    /// Adds `[/Pattern /DeviceGray]` under `name` to the resources (creating them if needed).
    fn add_pattern_gray(&mut self, resources: Option<&DictLoc>, owner: &DictLoc, name: &[u8]) {
        let resources = match resources {
            Some(loc) => loc.clone(),
            None => {
                let Some(dict) = dict_at_mut(&mut self.doc, owner) else {
                    return;
                };
                dict.set("Resources", Dictionary::new());
                let mut path = owner.path.clone();
                path.push(b"Resources".to_vec());
                DictLoc { id: owner.id, path }
            }
        };
        let spaces = match child(&self.doc, &resources, b"ColorSpace") {
            Some(loc) => loc,
            None => {
                let Some(dict) = dict_at_mut(&mut self.doc, &resources) else {
                    return;
                };
                dict.set("ColorSpace", Dictionary::new());
                let mut path = resources.path.clone();
                path.push(b"ColorSpace".to_vec());
                DictLoc {
                    id: resources.id,
                    path,
                }
            }
        };
        if let Some(dict) = dict_at_mut(&mut self.doc, &spaces) {
            dict.set(
                name.to_vec(),
                vec![
                    Object::Name(b"Pattern".to_vec()),
                    Object::Name(b"DeviceGray".to_vec()),
                ],
            );
        }
    }

    fn resources(&mut self, loc: &DictLoc, depth: usize) -> Result<(), SumiError> {
        if depth > self.settings.limits.max_depth {
            self.report
                .warn("resources nested too deeply were not converted");
            return Ok(());
        }
        if !self.done_dicts.insert(loc.clone()) {
            return Ok(());
        }
        let doc = &self.doc;
        let xobjects = child(doc, loc, b"XObject")
            .map(|l| children(doc, &l))
            .unwrap_or_default();
        let patterns = child(doc, loc, b"Pattern")
            .map(|l| children(doc, &l))
            .unwrap_or_default();
        let shadings = child(doc, loc, b"Shading")
            .map(|l| children(doc, &l))
            .unwrap_or_default();
        let fonts = child(doc, loc, b"Font")
            .map(|l| children(doc, &l))
            .unwrap_or_default();

        let xobjects: Vec<DictLoc> = xobjects
            .into_iter()
            .filter(|x| is_stream(&self.doc, x))
            .collect();
        for xobject in &xobjects {
            let subtype = dict_at(&self.doc, xobject)
                .and_then(|d| get_name(&self.doc, d, b"Subtype"))
                .map(<[u8]>::to_vec);
            match subtype.as_deref() {
                Some(b"Image") => {
                    if self.done_streams.insert(xobject.id) {
                        let result = crate::image::convert_xobject(
                            &mut self.doc,
                            xobject.id,
                            self.settings,
                            &mut self.report,
                        );
                        self.outcome("image", result)?;
                    }
                }
                Some(b"Form") => {
                    self.content_stream(xobject.id, Some(loc), depth + 1, "form XObject")?
                }
                _ => {}
            }
        }

        for pattern in &patterns {
            if !self.done_dicts.insert(pattern.clone()) {
                continue;
            }
            let pattern_type =
                dict_at(&self.doc, pattern).and_then(|d| get_int(&self.doc, d, b"PatternType"));
            match pattern_type {
                Some(1) if is_stream(&self.doc, pattern) => {
                    self.content_stream(pattern.id, Some(loc), depth + 1, "tiling pattern")?
                }
                Some(2) => {
                    if let Some(shading) = child(&self.doc, pattern, b"Shading") {
                        self.shading(&shading)?;
                    }
                }
                _ => {}
            }
        }

        for shading in &shadings {
            self.shading(shading)?;
        }

        for font in &fonts {
            let is_type3 = dict_at(&self.doc, font)
                .and_then(|d| get_name(&self.doc, d, b"Subtype"))
                == Some(b"Type3");
            if !is_type3 {
                continue;
            }
            let glyph_resources =
                child(&self.doc, font, b"Resources").unwrap_or_else(|| loc.clone());
            let procs: Vec<DictLoc> = child(&self.doc, font, b"CharProcs")
                .map(|l| children(&self.doc, &l))
                .unwrap_or_default()
                .into_iter()
                .filter(|g| is_stream(&self.doc, g))
                .collect();
            for glyph in &procs {
                self.content_stream(glyph.id, Some(&glyph_resources), depth + 1, "Type 3 glyph")?;
            }
        }
        Ok(())
    }

    /// Converts a form XObject, tiling pattern, glyph or appearance stream and its resources.
    fn content_stream(
        &mut self,
        id: ObjectId,
        fallback: Option<&DictLoc>,
        depth: usize,
        what: &str,
    ) -> Result<(), SumiError> {
        if !self.done_streams.insert(id) {
            return Ok(());
        }
        let loc = DictLoc::object(id);
        let resources = child(&self.doc, &loc, b"Resources").or_else(|| fallback.cloned());
        let Ok(Object::Stream(stream)) = self.doc.get_object(id) else {
            return Ok(());
        };
        let decoded = decode_stream(&self.doc, stream, self.settings.limits.max_stream_bytes);
        if let Some(data) = self.outcome(what, decoded)?
            && let Some((output, name)) = self.rewrite(&data, resources.as_ref(), true, what)?
        {
            if output.changed
                && let Ok(Object::Stream(stream)) = self.doc.get_object_mut(id)
            {
                set_flate_content(stream, &output.data);
                self.report.content_streams += 1;
            }
            if output.uses_pattern_gray {
                self.add_pattern_gray(resources.as_ref(), &loc, &name);
            }
        }
        self.group(&loc);
        let resources = child(&self.doc, &loc, b"Resources").or(resources);
        if let Some(resources) = resources {
            self.resources(&resources, depth)?;
        }
        Ok(())
    }

    fn shading(&mut self, loc: &DictLoc) -> Result<(), SumiError> {
        if !self.done_dicts.insert(loc.clone()) {
            return Ok(());
        }
        let Some(dict) = dict_at(&self.doc, loc) else {
            return Ok(());
        };
        let plan = crate::shading::plan(&self.doc, dict, self.settings);
        let Some(Some(plan)) = self.outcome("shading", plan)? else {
            return Ok(());
        };
        let function_id = self.doc.add_object(plan.function);
        if let Some(dict) = dict_at_mut(&mut self.doc, loc) {
            dict.set("ColorSpace", Object::Name(b"DeviceGray".to_vec()));
            dict.set("Function", Object::Reference(function_id));
            match plan.background {
                Some(background) => dict.set("Background", background),
                None => {
                    dict.remove(b"Background");
                }
            }
            self.report.shadings += 1;
        }
        Ok(())
    }

    /// Transparency groups blend in their own color space; make it gray as well.
    fn group(&mut self, owner: &DictLoc) {
        let Some(group) = child(&self.doc, owner, b"Group") else {
            return;
        };
        let needs_change = dict_at(&self.doc, &group).is_some_and(|g| {
            g.get(b"CS").is_ok_and(|cs| {
                let space =
                    ColorSpace::resolve(&self.doc, cs, None, self.settings.limits.max_stream_bytes);
                space.is_convertible() && !space.is_gray()
            })
        });
        if needs_change && let Some(dict) = dict_at_mut(&mut self.doc, &group) {
            dict.set("CS", Object::Name(b"DeviceGray".to_vec()));
        }
    }

    fn annotation(&mut self, loc: &DictLoc) -> Result<(), SumiError> {
        if !self.done_dicts.insert(loc.clone()) {
            return Ok(());
        }
        if let Some(appearance) = child(&self.doc, loc, b"AP") {
            for key in [&b"N"[..], b"R", b"D"] {
                let streams = match child(&self.doc, &appearance, key) {
                    Some(state) if is_stream(&self.doc, &state) => vec![state],
                    Some(states) => children(&self.doc, &states)
                        .into_iter()
                        .filter(|s| is_stream(&self.doc, s))
                        .collect(),
                    None => Vec::new(),
                };
                for stream in streams {
                    self.content_stream(stream.id, None, 1, "annotation appearance")?;
                }
            }
        }
        self.color_array(loc, b"C");
        self.color_array(loc, b"IC");
        if let Some(mk) = child(&self.doc, loc, b"MK") {
            self.color_array(&mk, b"BG");
            self.color_array(&mk, b"BC");
        }
        self.default_appearance(loc)
    }

    fn acroform(&mut self, acroform: &DictLoc) -> Result<(), SumiError> {
        self.default_appearance(acroform)?;
        if let Some(resources) = self.form_resources.clone() {
            self.resources(&resources, 0)?;
        }
        let mut stack: Vec<(ObjectId, usize)> = dict_at(&self.doc, acroform)
            .and_then(|d| d.get(b"Fields").ok())
            .map(|f| crate::objects::deref(&self.doc, f))
            .and_then(|f| f.as_array().ok())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|o| o.as_reference().ok())
                    .map(|id| (id, 0))
                    .collect()
            })
            .unwrap_or_default();
        while let Some((id, depth)) = stack.pop() {
            let loc = DictLoc::object(id);
            if depth > self.settings.limits.max_depth || self.done_dicts.contains(&loc) {
                continue;
            }
            let kids: Vec<ObjectId> = dict_at(&self.doc, &loc)
                .and_then(|d| d.get(b"Kids").ok())
                .map(|k| crate::objects::deref(&self.doc, k))
                .and_then(|k| k.as_array().ok())
                .map(|items| items.iter().filter_map(|o| o.as_reference().ok()).collect())
                .unwrap_or_default();
            self.annotation(&loc)?;
            stack.extend(kids.into_iter().map(|kid| (kid, depth + 1)));
        }
        Ok(())
    }

    /// Annotation colors such as `/C [1 0 0]`.
    fn color_array(&mut self, loc: &DictLoc, key: &[u8]) {
        let tone = self.settings.tone;
        let Some(values) = dict_at(&self.doc, loc)
            .and_then(|d| d.get(key).ok())
            .and_then(|a| a.as_array().ok())
            .and_then(|items| items.iter().map(number).collect::<Option<Vec<f64>>>())
        else {
            return;
        };
        let space = match values.len() {
            1 if tone.is_monochrome() => ColorSpace::Gray,
            3 => ColorSpace::Rgb,
            4 => ColorSpace::Cmyk,
            _ => return,
        };
        let gray = tone.apply(space.to_gray(&values).unwrap_or(0.0));
        if let Some(dict) = dict_at_mut(&mut self.doc, loc) {
            dict.set(key.to_vec(), vec![Object::Real(gray as f32)]);
        }
    }

    /// Default appearance strings (`/DA`) contain content stream operators such as `1 0 0 rg`.
    fn default_appearance(&mut self, loc: &DictLoc) -> Result<(), SumiError> {
        let Some(Object::String(da, _)) = dict_at(&self.doc, loc)
            .and_then(|d| d.get(b"DA").ok())
            .cloned()
        else {
            return Ok(());
        };
        let resources = self.form_resources.clone();
        let Some((output, _)) =
            self.rewrite(&da, resources.as_ref(), false, "default appearance")?
        else {
            return Ok(());
        };
        if output.changed
            && !output.uses_pattern_gray
            && let Some(dict) = dict_at_mut(&mut self.doc, loc)
        {
            dict.set("DA", Object::String(output.data, StringFormat::Literal));
        }
        Ok(())
    }
}
