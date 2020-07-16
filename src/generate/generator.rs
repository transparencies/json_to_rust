use super::item::{Field, Item, Struct};
use crate::{
    infer::{self, Map, Shape},
    util, Options,
};
use std::collections::HashSet;

type WrapId = Option<fn(&str) -> String>;

#[derive(Debug, Default)]
pub struct Generator {
    pub structs: Vec<Struct>,
    pub items: Vec<Item>,
    pub opts: Options,

    pub seen_structs: HashSet<String>,
    pub depth: usize,
}

impl Generator {
    const ANY_VALUE: &'static str = "::serde_json::Value";

    pub fn walk(&mut self, shape: &Shape, wrap: WrapId, name: &str) {
        self.depth += 1;

        match shape {
            Shape::Bottom | Shape::Any | Shape::Null => self.write_primitive(Self::ANY_VALUE, wrap),

            Shape::Bool => self.write_primitive("bool", wrap),
            Shape::String => self.write_primitive("String", wrap),
            Shape::Integer => self.write_primitive("i64", wrap),
            Shape::Float => self.write_primitive("f64", wrap),
            Shape::Opaque(ty) => self.write_primitive(ty, wrap),
            Shape::Optional(inner) => self.walk(inner, wrap, name),
            Shape::Array(ty) => self.make_vec(ty, name),
            Shape::Map(ty) => self.make_map(ty),

            Shape::Tuple(els, _) => {
                let folded = Shape::fold(els.clone());
                // eprintln!("folded: [{}; {}]", folded.root(), e);
                if folded == Shape::Any && els.iter().any(|s| *s != Shape::Any) {
                    self.make_tuple(els, None)
                } else {
                    self.make_vec(&folded, name)
                }
            }

            Shape::Object(ty) => self.make_struct(name, ty, None),
        }

        self.depth -= 1;
    }

    fn make_tuple(&mut self, shapes: &[Shape], wrap: WrapId) {
        let (mut types, mut defs) = (String::new(), Vec::new());
        for shape in shapes {
            self.walk(shape, wrap, "");
            if !types.is_empty() {
                types.push_str(", ");
            }

            let last = self.items.pop().unwrap();
            types.push_str(&last.ident);

            for last in last.body {
                if !defs.is_empty() {
                    defs.push("\n".into());
                }
                defs.push(last);
            }
        }

        self.items.push(Item {
            ident: format!("({})", types),
            body: defs,
        });
    }

    fn make_struct(&mut self, input_name: &str, map: &Map, wrap: WrapId) {
        let struct_name = util::fix_struct_name(input_name, &mut self.seen_structs);

        let mut defs = Vec::new();
        let mut body = Vec::new();

        let mut seen_fields = HashSet::new();

        for (name, shape) in map.iter().rev() {
            let field_name = util::fix_field_name(name, &mut seen_fields);
            let field_renamed = field_name != *name;

            match shape {
                Shape::Object(map) => {
                    if self.opts.max_size.is_none() {
                        self.make_field_map(map)
                    } else if self.opts.max_size.filter(|&max| map.len() > max).is_some() {
                        self.make_field_map(map);
                    } else {
                        self.walk(shape, wrap, &field_name)
                    }
                }
                Shape::Map(_) => panic!("shouldn't have a map here"),
                _ => self.walk(shape, wrap, &field_name),
            }

            let item = self.items.pop().unwrap();
            defs.extend(item.body);
            body.push(Field {
                rename: if field_renamed {
                    Some(name.clone())
                } else {
                    None
                },
                binding: field_name,
                kind: item.ident,
            });
        }

        self.structs.push(Struct {
            rename: self
                .opts
                .json_name
                .as_ref()
                .filter(|_| self.depth == 1)
                .map(Clone::clone),
            name: struct_name.clone(),
            fields: body,
        });

        self.items.push(Item {
            ident: struct_name,
            body: defs,
        });
    }

    fn make_field_map(&mut self, map: &Map) {
        let shape = infer::Shape::fold(map.values().cloned());
        let local = infer::Local::new(shape);

        let mut ident = String::from("::std::collections::HashMap<String, ");
        local.format(&mut ident);
        ident.push('>');

        self.items.push(Item {
            ident,
            body: vec![],
        })
    }

    fn make_map(&mut self, ty: &Shape) {
        self.walk(
            ty,
            Some(|s| format!("::std::collections::HashMap<String, {}>", s)),
            "",
        );
    }

    fn make_vec(&mut self, ty: &Shape, name: &str) {
        self.walk(ty, Some(|s| format!("::std::vec::Vec<{}>", s)), name);
    }

    fn write_primitive(&mut self, s: impl Into<String>, wrap: WrapId) {
        let s = s.into();
        self.items.push(Item {
            ident: wrap.map(|w| w(&s)).unwrap_or_else(|| s),
            body: vec![],
        });
    }
}