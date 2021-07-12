use std::collections::BTreeMap;
use std::fs::File;
use std::iter::Peekable;
use std::ops::Index;
use std::str::FromStr;

use libyaml::Event;
use libyaml::Parser;
use libyaml::ParserIter;

pub type YamlMap = BTreeMap<String, BoxedYamlElement>;
pub type YamlSet = Vec<BoxedYamlElement>;
type BoxedYamlElement = Box<YamlElement>;

#[derive(Debug)]
pub struct YamlDocument {
    root: YamlSet,
    anchor: YamlMap,
}

impl std::ops::Deref for YamlDocument {
    type Target = YamlSet;

    fn deref(&self) -> &Self::Target {
        &self.root
    }
}

#[derive(Debug, Clone, Ord, Eq, PartialEq, PartialOrd)]
pub enum YamlElement {
    Scalar(String, Option<String>),
    Map(YamlMap, Option<String>),
    Set(YamlSet, Option<String>),
    Alias(String),
    None,
}

impl YamlElement {
    pub fn as_scalar<T>(&self) -> Option<T>
    where
        T: FromStr,
    {
        if let Self::Scalar(s, _) = self {
            Some(s.parse().ok()?)
        } else {
            None
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        if let Self::Scalar(s, _) = self {
            Some(s)
        } else {
            None
        }
    }

    pub fn as_map(&self) -> Option<&YamlMap> {
        if let Self::Map(map, _) = self {
            Some(map)
        } else {
            None
        }
    }

    pub fn as_vec(&self) -> Option<YamlSet> {
        if let Self::Set(map, _) = self {
            Some(map.clone())
        } else if self == &Self::None {
            Some(YamlSet::new())
        } else {
            None
        }
    }

    pub fn get_tag(&self) -> Option<String> {
        match self {
            YamlElement::Scalar(_, s) | YamlElement::Map(_, s) | YamlElement::Set(_, s) => {
                s.clone()
            }
            YamlElement::Alias(_) | &YamlElement::None => None,
        }
    }
}

impl YamlDocument {
    pub fn new<'a>(path: impl Into<&'a str>) -> Option<Self> {
        let file = File::open(path.into()).ok()?;
        let parser = Parser::new(file).ok()?;
        let iter = &mut parser.into_iter().peekable();
        let mut s = Self {
            root: vec![],
            anchor: BTreeMap::new(),
        };
        while let Some(Ok(i)) = iter.peek() {
            println!("{:?}", i);
            match i {
                Event::StreamStart { .. } => {
                    iter.next();
                }
                Event::DocumentStart { .. } => {
                    iter.next();
                }
                Event::MappingStart { .. } => {
                    let map = s.map(iter)?;
                    s.root.push(map);
                    iter.next();
                }
                Event::SequenceStart { .. } => {
                    let vec = s.sequence(iter)?;
                    s.root.push(vec);
                    iter.next();
                }
                Event::DocumentEnd { .. } => {
                    iter.next();
                }
                Event::StreamEnd => {
                    //self.resolve_alias();
                    return Some(s);
                }
                _ => unreachable!("{:?}", i),
            }
        }
        None
    }

    pub fn resolve_alias(&self, alias: &YamlElement) -> Option<BoxedYamlElement> {
        if let YamlElement::Alias(alias) = alias {
            if self.anchor.contains_key(alias) {
                return Some(self.anchor[alias].clone());
            }
        }
        None
    }

    fn scalar(&mut self, iter: &mut Peekable<ParserIter>) -> Option<BoxedYamlElement> {
        if let Some(Ok(Event::Scalar {
            value, anchor, tag, ..
        })) = iter.peek()
        {
            let scalar = Box::new(YamlElement::Scalar(value.clone(), tag.clone()));
            if let Some(anchor) = anchor {
                self.anchor.insert(anchor.clone(), scalar.clone());
            }
            return Some(scalar);
        }
        None
    }

    fn sequence(&mut self, iter: &mut Peekable<ParserIter>) -> Option<BoxedYamlElement> {
        let el = iter.next()?.ok()?;
        let mut root = YamlSet::new();
        while let Some(Ok(i)) = iter.peek() {
            println!("{:?}", i);
            match i {
                Event::Scalar { .. } => {
                    root.push(self.scalar(iter)?);
                    iter.next();
                }
                Event::SequenceStart { .. } => {
                    root.push(self.sequence(iter)?);
                    iter.next();
                }
                Event::MappingStart { .. } => {
                    root.push(self.map(iter)?);
                    iter.next();
                }
                Event::Alias { anchor } => {
                    root.push(Box::new(YamlElement::Alias(anchor.clone())));
                    iter.next();
                }
                Event::SequenceEnd => {
                    if let Event::SequenceStart { anchor, tag, .. } = el {
                        let root = Box::new(YamlElement::Set(root, tag));
                        if let Some(anchor) = anchor {
                            self.anchor.insert(anchor, root.clone());
                        }
                        return Some(root);
                    }
                    unreachable!()
                }
                _ => unreachable!("{:?}", i),
            }
        }
        None
    }

    fn map(&mut self, iter: &mut Peekable<ParserIter>) -> Option<BoxedYamlElement> {
        let el = iter.next()?.ok()?;
        let mut map = YamlMap::new();
        let mut is_key = true;
        let mut key = None;
        while let Some(Ok(i)) = iter.peek() {
            println!("{:?}", i);
            match i {
                Event::Scalar { .. } => {
                    println!("{} {:?}", is_key, key);
                    if is_key {
                        if let YamlElement::Scalar(value, _) = *self.scalar(iter)? {
                            key = Some(value);
                            is_key = false
                        } else {
                            return None;
                        }
                    } else {
                        map.insert(key.clone()?, self.scalar(iter)?);
                        is_key = true;
                    }
                    iter.next();
                }
                Event::MappingStart { .. } => {
                    map.insert(key.clone()?, self.map(iter)?);
                    is_key = true;
                    iter.next();
                }
                Event::SequenceStart { .. } => {
                    map.insert(key.clone()?, self.sequence(iter)?);
                    is_key = true;
                    iter.next();
                }
                Event::Alias { anchor } => {
                    map.insert(key.clone()?, Box::new(YamlElement::Alias(anchor.clone())));
                    is_key = true;
                    iter.next();
                }
                Event::MappingEnd => {
                    if let Event::MappingStart { anchor, tag, .. } = el {
                        let map = Box::new(YamlElement::Map(map, tag));
                        if let Some(anchor) = anchor {
                            self.anchor.insert(anchor, map.clone());
                        }
                        return Some(map);
                    }
                    unreachable!()
                }
                _ => unreachable!("{:?}", i),
            }
        }
        None
    }
}

impl Index<&str> for YamlElement {
    type Output = YamlElement;

    fn index(&self, index: &str) -> &Self::Output {
        match self.as_map() {
            Some(map) if map.contains_key(index) => map[index].as_ref(),
            _ => &Self::None,
        }
    }
}

impl Index<String> for YamlElement {
    type Output = YamlElement;

    fn index(&self, index: String) -> &Self::Output {
        match self.as_map() {
            Some(map) if map.contains_key(&index) => map[&index].as_ref(),
            _ => &Self::None,
        }
    }
}
