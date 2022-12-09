use crate::value::Datatype;
use crate::Automerge;
use automerge as am;
use automerge::transaction::Transactable;
use automerge::{Change, ChangeHash, ObjType, Prop};
use js_sys::{Array, Function, Object, Reflect, Symbol, Uint8Array};
use std::collections::{BTreeSet, HashSet};
use std::fmt::Display;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::{observer::Patch, ObjId, Value};

const RAW_DATA_SYMBOL: &str = "_am_raw_value_";
const DATATYPE_SYMBOL: &str = "_am_datatype_";
const RAW_OBJECT_SYMBOL: &str = "_am_objectId";
const META_SYMBOL: &str = "_am_meta";

pub(crate) struct JS(pub(crate) JsValue);
pub(crate) struct AR(pub(crate) Array);

impl From<AR> for JsValue {
    fn from(ar: AR) -> Self {
        ar.0.into()
    }
}

impl From<JS> for JsValue {
    fn from(js: JS) -> Self {
        js.0
    }
}

impl From<am::sync::State> for JS {
    fn from(state: am::sync::State) -> Self {
        let shared_heads: JS = state.shared_heads.into();
        let last_sent_heads: JS = state.last_sent_heads.into();
        let their_heads: JS = state.their_heads.into();
        let their_need: JS = state.their_need.into();
        let sent_hashes: JS = state.sent_hashes.into();
        let their_have = if let Some(have) = &state.their_have {
            JsValue::from(AR::from(have.as_slice()).0)
        } else {
            JsValue::null()
        };
        let result: JsValue = Object::new().into();
        // we can unwrap here b/c we made the object and know its not frozen
        Reflect::set(&result, &"sharedHeads".into(), &shared_heads.0).unwrap();
        Reflect::set(&result, &"lastSentHeads".into(), &last_sent_heads.0).unwrap();
        Reflect::set(&result, &"theirHeads".into(), &their_heads.0).unwrap();
        Reflect::set(&result, &"theirNeed".into(), &their_need.0).unwrap();
        Reflect::set(&result, &"theirHave".into(), &their_have).unwrap();
        Reflect::set(&result, &"sentHashes".into(), &sent_hashes.0).unwrap();
        Reflect::set(&result, &"inFlight".into(), &state.in_flight.into()).unwrap();
        JS(result)
    }
}

impl From<Vec<ChangeHash>> for JS {
    fn from(heads: Vec<ChangeHash>) -> Self {
        JS(heads
            .iter()
            .map(|h| JsValue::from_str(&h.to_string()))
            .collect::<Array>()
            .into())
    }
}

impl From<HashSet<ChangeHash>> for JS {
    fn from(heads: HashSet<ChangeHash>) -> Self {
        let result: JsValue = Object::new().into();
        for key in &heads {
            Reflect::set(&result, &key.to_string().into(), &true.into()).unwrap();
        }
        JS(result)
    }
}

impl From<BTreeSet<ChangeHash>> for JS {
    fn from(heads: BTreeSet<ChangeHash>) -> Self {
        let result: JsValue = Object::new().into();
        for key in &heads {
            Reflect::set(&result, &key.to_string().into(), &true.into()).unwrap();
        }
        JS(result)
    }
}

impl From<Option<Vec<ChangeHash>>> for JS {
    fn from(heads: Option<Vec<ChangeHash>>) -> Self {
        if let Some(v) = heads {
            let v: Array = v
                .iter()
                .map(|h| JsValue::from_str(&h.to_string()))
                .collect();
            JS(v.into())
        } else {
            JS(JsValue::null())
        }
    }
}

impl TryFrom<JS> for HashSet<ChangeHash> {
    type Error = error::BadChangeHashSet;

    fn try_from(value: JS) -> Result<Self, Self::Error> {
        let result = HashSet::new();
        fold_hash_set(result, &value.0, |mut set, hash| {
            set.insert(hash);
            set
        })
    }
}

impl TryFrom<JS> for BTreeSet<ChangeHash> {
    type Error = error::BadChangeHashSet;

    fn try_from(value: JS) -> Result<Self, Self::Error> {
        let result = BTreeSet::new();
        fold_hash_set(result, &value.0, |mut set, hash| {
            set.insert(hash);
            set
        })
    }
}

fn fold_hash_set<F, O>(init: O, val: &JsValue, f: F) -> Result<O, error::BadChangeHashSet>
where
    F: Fn(O, ChangeHash) -> O,
{
    let mut result = init;
    for key in Reflect::own_keys(val)
        .map_err(|_| error::BadChangeHashSet::ListProp)?
        .iter()
    {
        if let Some(true) = js_get(val, &key)?.0.as_bool() {
            let hash = ChangeHash::try_from(JS(key.clone()))
                .map_err(|e| error::BadChangeHashSet::BadHash(key, e))?;
            result = f(result, hash);
        }
    }
    Ok(result)
}

impl TryFrom<JS> for ChangeHash {
    type Error = error::BadChangeHash;

    fn try_from(value: JS) -> Result<Self, Self::Error> {
        if let Some(s) = value.0.as_string() {
            Ok(s.parse()?)
        } else {
            Err(error::BadChangeHash::NotString)
        }
    }
}

impl TryFrom<JS> for Option<Vec<ChangeHash>> {
    type Error = error::BadChangeHashes;

    fn try_from(value: JS) -> Result<Self, Self::Error> {
        if value.0.is_null() {
            Ok(None)
        } else {
            Vec::<ChangeHash>::try_from(value).map(Some)
        }
    }
}

impl TryFrom<JS> for Vec<ChangeHash> {
    type Error = error::BadChangeHashes;

    fn try_from(value: JS) -> Result<Self, Self::Error> {
        let value = value
            .0
            .dyn_into::<Array>()
            .map_err(|_| error::BadChangeHashes::NotArray)?;
        let value = value
            .iter()
            .enumerate()
            .map(|(i, v)| {
                ChangeHash::try_from(JS(v)).map_err(|e| error::BadChangeHashes::BadElem(i, e))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(value)
    }
}

impl TryFrom<JS> for Vec<Change> {
    type Error = error::BadJSChanges;

    fn try_from(value: JS) -> Result<Self, Self::Error> {
        let value = value
            .0
            .dyn_into::<Array>()
            .map_err(|_| error::BadJSChanges::ChangesNotArray)?;
        let changes = value
            .iter()
            .enumerate()
            .map(|(i, j)| {
                j.dyn_into().map_err::<error::BadJSChanges, _>(|_| {
                    error::BadJSChanges::ElemNotUint8Array(i)
                })
            })
            .collect::<Result<Vec<Uint8Array>, _>>()?;
        let changes = changes
            .iter()
            .enumerate()
            .map(|(i, arr)| {
                automerge::Change::try_from(arr.to_vec().as_slice())
                    .map_err(|e| error::BadJSChanges::BadChange(i, e))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(changes)
    }
}

impl TryFrom<JS> for am::sync::State {
    type Error = error::BadSyncState;

    fn try_from(value: JS) -> Result<Self, Self::Error> {
        let value = value.0;
        let shared_heads = js_get(&value, "sharedHeads")?
            .try_into()
            .map_err(error::BadSyncState::BadSharedHeads)?;
        let last_sent_heads = js_get(&value, "lastSentHeads")?
            .try_into()
            .map_err(error::BadSyncState::BadLastSentHeads)?;
        let their_heads = js_get(&value, "theirHeads")?
            .try_into()
            .map_err(error::BadSyncState::BadTheirHeads)?;
        let their_need = js_get(&value, "theirNeed")?
            .try_into()
            .map_err(error::BadSyncState::BadTheirNeed)?;
        let their_have = js_get(&value, "theirHave")?
            .try_into()
            .map_err(error::BadSyncState::BadTheirHave)?;
        let sent_hashes = js_get(&value, "sentHashes")?
            .try_into()
            .map_err(error::BadSyncState::BadSentHashes)?;
        let in_flight = js_get(&value, "inFlight")?
            .0
            .as_bool()
            .ok_or(error::BadSyncState::InFlightNotBoolean)?;
        Ok(am::sync::State {
            shared_heads,
            last_sent_heads,
            their_heads,
            their_need,
            their_have,
            sent_hashes,
            in_flight,
        })
    }
}

impl TryFrom<JS> for am::sync::Have {
    type Error = error::BadHave;

    fn try_from(value: JS) -> Result<Self, Self::Error> {
        let last_sync = js_get(&value.0, "lastSync")?
            .try_into()
            .map_err(error::BadHave::BadLastSync)?;
        let bloom = js_get(&value.0, "bloom")?
            .try_into()
            .map_err(error::BadHave::BadBloom)?;
        Ok(am::sync::Have { last_sync, bloom })
    }
}

impl TryFrom<JS> for Option<Vec<am::sync::Have>> {
    type Error = error::BadHaves;

    fn try_from(value: JS) -> Result<Self, Self::Error> {
        if value.0.is_null() {
            Ok(None)
        } else {
            Ok(Some(value.try_into()?))
        }
    }
}

impl TryFrom<JS> for Vec<am::sync::Have> {
    type Error = error::BadHaves;

    fn try_from(value: JS) -> Result<Self, Self::Error> {
        let value = value
            .0
            .dyn_into::<Array>()
            .map_err(|_| error::BadHaves::NotArray)?;
        let have = value
            .iter()
            .enumerate()
            .map(|(i, s)| JS(s).try_into().map_err(|e| error::BadHaves::BadElem(i, e)))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(have)
    }
}

impl TryFrom<JS> for am::sync::BloomFilter {
    type Error = error::BadBloom;

    fn try_from(value: JS) -> Result<Self, Self::Error> {
        let value: Uint8Array = value
            .0
            .dyn_into()
            .map_err(|_| error::BadBloom::NotU8Array)?;
        let value = value.to_vec();
        let value = value.as_slice().try_into()?;
        Ok(value)
    }
}

impl TryFrom<JS> for am::sync::Message {
    type Error = error::BadSyncMessage;

    fn try_from(value: JS) -> Result<Self, Self::Error> {
        let heads = js_get(&value.0, "heads")?
            .try_into()
            .map_err(error::BadSyncMessage::BadHeads)?;
        let need = js_get(&value.0, "need")?
            .try_into()
            .map_err(error::BadSyncMessage::BadNeed)?;
        let changes = js_get(&value.0, "changes")?.try_into()?;
        let have = js_get(&value.0, "have")?.try_into()?;
        Ok(am::sync::Message {
            heads,
            need,
            have,
            changes,
        })
    }
}

impl From<&[ChangeHash]> for AR {
    fn from(value: &[ChangeHash]) -> Self {
        AR(value
            .iter()
            .map(|h| JsValue::from_str(&hex::encode(h.0)))
            .collect())
    }
}

impl From<&[Change]> for AR {
    fn from(value: &[Change]) -> Self {
        let changes: Array = value
            .iter()
            .map(|c| Uint8Array::from(c.raw_bytes()))
            .collect();
        AR(changes)
    }
}

impl From<&[am::sync::Have]> for AR {
    fn from(value: &[am::sync::Have]) -> Self {
        AR(value
            .iter()
            .map(|have| {
                let last_sync: Array = have
                    .last_sync
                    .iter()
                    .map(|h| JsValue::from_str(&hex::encode(h.0)))
                    .collect();
                // FIXME - the clone and the unwrap here shouldnt be needed - look at into_bytes()
                let bloom = Uint8Array::from(have.bloom.to_bytes().as_slice());
                let obj: JsValue = Object::new().into();
                // we can unwrap here b/c we created the object and know its not frozen
                Reflect::set(&obj, &"lastSync".into(), &last_sync.into()).unwrap();
                Reflect::set(&obj, &"bloom".into(), &bloom.into()).unwrap();
                obj
            })
            .collect())
    }
}

pub(crate) fn to_js_err<T: Display>(err: T) -> JsValue {
    js_sys::Error::new(&std::format!("{}", err)).into()
}

pub(crate) fn js_get<J: Into<JsValue>, S: std::fmt::Debug + Into<JsValue>>(
    obj: J,
    prop: S,
) -> Result<JS, error::GetProp> {
    let prop = prop.into();
    Ok(JS(Reflect::get(&obj.into(), &prop).map_err(|e| {
        error::GetProp {
            property: format!("{:?}", prop),
            error: e,
        }
    })?))
}

pub(crate) fn js_set<V: Into<JsValue>, S: std::fmt::Debug + Into<JsValue>>(
    obj: &JsValue,
    prop: S,
    val: V,
) -> Result<bool, error::SetProp> {
    let prop = prop.into();
    Reflect::set(obj, &prop, &val.into()).map_err(|e| error::SetProp {
        property: prop,
        error: e,
    })
}

pub(crate) fn js_get_symbol<J: Into<JsValue>>(obj: J, prop: &Symbol) -> Result<JS, error::GetProp> {
    Ok(JS(Reflect::get(&obj.into(), &prop.into()).map_err(
        |e| error::GetProp {
            property: format!("{}", prop.to_string()),
            error: e,
        },
    )?))
}

pub(crate) fn to_prop(p: JsValue) -> Result<Prop, super::error::InvalidProp> {
    if let Some(s) = p.as_string() {
        Ok(Prop::Map(s))
    } else if let Some(n) = p.as_f64() {
        Ok(Prop::Seq(n as usize))
    } else {
        Err(super::error::InvalidProp)
    }
}

pub(crate) fn to_objtype(
    value: &JsValue,
    datatype: &Option<String>,
) -> Option<(ObjType, Vec<(Prop, JsValue)>)> {
    match datatype.as_deref() {
        Some("map") => {
            let map = value.clone().dyn_into::<js_sys::Object>().ok()?;
            let map = js_sys::Object::keys(&map)
                .iter()
                .zip(js_sys::Object::values(&map).iter())
                .map(|(key, val)| (key.as_string().unwrap().into(), val))
                .collect();
            Some((ObjType::Map, map))
        }
        Some("list") => {
            let list = value.clone().dyn_into::<js_sys::Array>().ok()?;
            let list = list
                .iter()
                .enumerate()
                .map(|(i, e)| (i.into(), e))
                .collect();
            Some((ObjType::List, list))
        }
        Some("text") => {
            let text = value.as_string()?;
            let text = text
                .chars()
                .enumerate()
                .map(|(i, ch)| (i.into(), ch.to_string().into()))
                .collect();
            Some((ObjType::Text, text))
        }
        Some(_) => None,
        None => {
            if let Ok(list) = value.clone().dyn_into::<js_sys::Array>() {
                let list = list
                    .iter()
                    .enumerate()
                    .map(|(i, e)| (i.into(), e))
                    .collect();
                Some((ObjType::List, list))
            } else if let Ok(map) = value.clone().dyn_into::<js_sys::Object>() {
                // FIXME unwrap
                let map = js_sys::Object::keys(&map)
                    .iter()
                    .zip(js_sys::Object::values(&map).iter())
                    .map(|(key, val)| (key.as_string().unwrap().into(), val))
                    .collect();
                Some((ObjType::Map, map))
            } else if let Some(text) = value.as_string() {
                let text = text
                    .chars()
                    .enumerate()
                    .map(|(i, ch)| (i.into(), ch.to_string().into()))
                    .collect();
                Some((ObjType::Text, text))
            } else {
                None
            }
        }
    }
}

pub(crate) fn get_heads(
    heads: Option<Array>,
) -> Result<Option<Vec<ChangeHash>>, error::BadChangeHashes> {
    heads
        .map(|h| {
            h.iter()
                .enumerate()
                .map(|(i, v)| {
                    ChangeHash::try_from(JS(v)).map_err(|e| error::BadChangeHashes::BadElem(i, e))
                })
                .collect()
        })
        .transpose()
}

impl Automerge {
    pub(crate) fn export_object(
        &self,
        obj: &ObjId,
        datatype: Datatype,
        heads: Option<&Vec<ChangeHash>>,
        meta: &JsValue,
    ) -> Result<JsValue, error::Export> {
        let result = if datatype.is_sequence() {
            self.wrap_object(
                self.export_list(obj, heads, meta)?,
                datatype,
                &obj.to_string().into(),
                meta,
            )?
        } else {
            self.wrap_object(
                self.export_map(obj, heads, meta)?,
                datatype,
                &obj.to_string().into(),
                meta,
            )?
        };
        Ok(result.into())
    }

    pub(crate) fn export_map(
        &self,
        obj: &ObjId,
        heads: Option<&Vec<ChangeHash>>,
        meta: &JsValue,
    ) -> Result<Object, error::Export> {
        let keys = self.doc.keys(obj);
        let map = Object::new();
        for k in keys {
            let val_and_id = if let Some(heads) = heads {
                self.doc.get_at(obj, &k, heads)
            } else {
                self.doc.get(obj, &k)
            };
            if let Ok(Some((val, id))) = val_and_id {
                let subval = match val {
                    Value::Object(o) => self.export_object(&id, o.into(), heads, meta)?,
                    Value::Scalar(_) => self.export_value(alloc(&val))?,
                };
                js_set(&map, &k, &subval)?;
            };
        }

        Ok(map)
    }

    pub(crate) fn export_list(
        &self,
        obj: &ObjId,
        heads: Option<&Vec<ChangeHash>>,
        meta: &JsValue,
    ) -> Result<Object, error::Export> {
        let len = self.doc.length(obj);
        let array = Array::new();
        for i in 0..len {
            let val_and_id = if let Some(heads) = heads {
                self.doc.get_at(obj, i as usize, heads)
            } else {
                self.doc.get(obj, i as usize)
            };
            if let Ok(Some((val, id))) = val_and_id {
                let subval = match val {
                    Value::Object(o) => self.export_object(&id, o.into(), heads, meta)?,
                    Value::Scalar(_) => self.export_value(alloc(&val))?,
                };
                array.push(&subval);
            };
        }

        Ok(array.into())
    }

    pub(crate) fn export_value(
        &self,
        (datatype, raw_value): (Datatype, JsValue),
    ) -> Result<JsValue, error::Export> {
        if let Some(function) = self.external_types.get(&datatype) {
            let wrapped_value = function
                .call1(&JsValue::undefined(), &raw_value)
                .map_err(|e| error::Export::CallDataHandler(datatype.to_string(), e))?;
            if let Ok(o) = wrapped_value.dyn_into::<Object>() {
                let key = Symbol::for_(RAW_DATA_SYMBOL);
                set_hidden_value(&o, &key, &raw_value)?;
                let key = Symbol::for_(DATATYPE_SYMBOL);
                set_hidden_value(&o, &key, datatype)?;
                Ok(o.into())
            } else {
                Err(error::Export::InvalidDataHandler(datatype.to_string()))
            }
        } else {
            Ok(raw_value)
        }
    }

    pub(crate) fn unwrap_object(
        &self,
        ext_val: &Object,
    ) -> Result<(Object, Datatype, JsValue), error::Export> {
        let inner = js_get_symbol(ext_val, &Symbol::for_(RAW_DATA_SYMBOL))?.0;

        let datatype = js_get_symbol(ext_val, &Symbol::for_(DATATYPE_SYMBOL))?
            .0
            .try_into();

        let mut id = js_get_symbol(ext_val, &Symbol::for_(RAW_OBJECT_SYMBOL))?.0;
        if id.is_undefined() {
            id = "_root".into();
        }

        let inner = inner
            .dyn_into::<Object>()
            .unwrap_or_else(|_| ext_val.clone());
        let datatype = datatype.unwrap_or_else(|_| {
            if Array::is_array(&inner) {
                Datatype::List
            } else {
                Datatype::Map
            }
        });
        Ok((inner, datatype, id))
    }

    pub(crate) fn unwrap_scalar(&self, ext_val: JsValue) -> Result<JsValue, error::Export> {
        let inner = js_get_symbol(&ext_val, &Symbol::for_(RAW_DATA_SYMBOL))?.0;
        if !inner.is_undefined() {
            Ok(inner)
        } else {
            Ok(ext_val)
        }
    }

    fn maybe_wrap_object(
        &self,
        (datatype, raw_value): (Datatype, JsValue),
        id: &ObjId,
        meta: &JsValue,
    ) -> Result<JsValue, error::Export> {
        if let Ok(obj) = raw_value.clone().dyn_into::<Object>() {
            let result = self.wrap_object(obj, datatype, &id.to_string().into(), meta)?;
            Ok(result.into())
        } else {
            self.export_value((datatype, raw_value))
        }
    }

    pub(crate) fn wrap_object(
        &self,
        value: Object,
        datatype: Datatype,
        id: &JsValue,
        meta: &JsValue,
    ) -> Result<Object, error::Export> {
        let value = if let Some(function) = self.external_types.get(&datatype) {
            let wrapped_value = function
                .call1(&JsValue::undefined(), &value)
                .map_err(|e| error::Export::CallDataHandler(datatype.to_string(), e))?;
            let wrapped_object = wrapped_value
                .dyn_into::<Object>()
                .map_err(|_| error::Export::InvalidDataHandler(datatype.to_string()))?;
            set_hidden_value(&wrapped_object, &Symbol::for_(RAW_DATA_SYMBOL), value)?;
            wrapped_object
        } else {
            value
        };
        if matches!(datatype, Datatype::Map | Datatype::List | Datatype::Text) {
            set_hidden_value(&value, &Symbol::for_(RAW_OBJECT_SYMBOL), id)?;
        }
        set_hidden_value(&value, &Symbol::for_(DATATYPE_SYMBOL), datatype)?;
        set_hidden_value(&value, &Symbol::for_(META_SYMBOL), meta)?;
        if self.freeze {
            Object::freeze(&value);
        }
        Ok(value)
    }

    pub(crate) fn apply_patch_to_array(
        &self,
        array: &Object,
        patch: &Patch,
        meta: &JsValue,
    ) -> Result<Object, error::ApplyPatch> {
        let result = Array::from(array); // shallow copy
        match patch {
            Patch::PutSeq { index, value, .. } => {
                let sub_val = self.maybe_wrap_object(alloc(&value.0), &value.1, meta)?;
                js_set(&result, *index as f64, &sub_val)?;
                Ok(result.into())
            }
            Patch::DeleteSeq { index, .. } => {
                Ok(self.sub_splice(result, *index, 1, vec![], meta)?)
            }
            Patch::Insert { index, values, .. } => {
                Ok(self.sub_splice(result, *index, 0, values, meta)?)
            }
            Patch::Increment { prop, value, .. } => {
                if let Prop::Seq(index) = prop {
                    let index = *index as f64;
                    let old_val = js_get(&result, index)?.0;
                    let old_val = self.unwrap_scalar(old_val)?;
                    if let Some(old) = old_val.as_f64() {
                        let new_value: Value<'_> =
                            am::ScalarValue::counter(old as i64 + *value).into();
                        js_set(&result, index, &self.export_value(alloc(&new_value))?)?;
                        Ok(result.into())
                    } else {
                        Err(error::ApplyPatch::IncrementNonNumeric)
                    }
                } else {
                    Err(error::ApplyPatch::IncrementKeyInSeq)
                }
            }
            Patch::DeleteMap { .. } => Err(error::ApplyPatch::DeleteKeyFromSeq),
            Patch::PutMap { .. } => Err(error::ApplyPatch::PutKeyInSeq),
        }
    }

    pub(crate) fn apply_patch_to_map(
        &self,
        map: &Object,
        patch: &Patch,
        meta: &JsValue,
    ) -> Result<Object, error::ApplyPatch> {
        let result = Object::assign(&Object::new(), map); // shallow copy
        match patch {
            Patch::PutMap { key, value, .. } => {
                let sub_val = self.maybe_wrap_object(alloc(&value.0), &value.1, meta)?;
                js_set(&result, key, &sub_val)?;
                Ok(result)
            }
            Patch::DeleteMap { key, .. } => {
                Reflect::delete_property(&result, &key.into()).map_err(|e| {
                    error::Export::Delete {
                        prop: key.to_string(),
                        err: e,
                    }
                })?;
                Ok(result)
            }
            Patch::Increment { prop, value, .. } => {
                if let Prop::Map(key) = prop {
                    let old_val = js_get(&result, key)?.0;
                    let old_val = self.unwrap_scalar(old_val)?;
                    if let Some(old) = old_val.as_f64() {
                        let new_value: Value<'_> =
                            am::ScalarValue::counter(old as i64 + *value).into();
                        js_set(&result, key, &self.export_value(alloc(&new_value))?)?;
                        Ok(result)
                    } else {
                        Err(error::ApplyPatch::IncrementNonNumeric)
                    }
                } else {
                    Err(error::ApplyPatch::IncrementIndexInMap)
                }
            }
            Patch::Insert { .. } => Err(error::ApplyPatch::InsertInMap),
            Patch::DeleteSeq { .. } => Err(error::ApplyPatch::SpliceInMap),
            Patch::PutSeq { .. } => Err(error::ApplyPatch::PutIdxInMap),
        }
    }

    pub(crate) fn apply_patch(
        &self,
        obj: Object,
        patch: &Patch,
        depth: usize,
        meta: &JsValue,
    ) -> Result<Object, error::ApplyPatch> {
        let (inner, datatype, id) = self.unwrap_object(&obj)?;
        let prop = patch.path().get(depth).map(|p| prop_to_js(&p.1));
        let result = if let Some(prop) = prop {
            if let Ok(sub_obj) = js_get(&inner, &prop)?.0.dyn_into::<Object>() {
                let new_value = self.apply_patch(sub_obj, patch, depth + 1, meta)?;
                let result = shallow_copy(&inner);
                js_set(&result, &prop, &new_value)?;
                Ok(result)
            } else {
                // if a patch is trying to access a deleted object make no change
                // short circuit the wrap process
                return Ok(obj);
            }
        } else if Array::is_array(&inner) {
            self.apply_patch_to_array(&inner, patch, meta)
        } else {
            self.apply_patch_to_map(&inner, patch, meta)
        }?;

        self.wrap_object(result, datatype, &id, meta)
            .map_err(|e| e.into())
    }

    fn sub_splice<'a, I: IntoIterator<Item = &'a (Value<'a>, ObjId)>>(
        &self,
        o: Array,
        index: usize,
        num_del: usize,
        values: I,
        meta: &JsValue,
    ) -> Result<Object, error::Export> {
        let args: Array = values
            .into_iter()
            .map(|v| self.maybe_wrap_object(alloc(&v.0), &v.1, meta))
            .collect::<Result<_, _>>()?;
        args.unshift(&(num_del as u32).into());
        args.unshift(&(index as u32).into());
        let method = js_get(&o, "splice")?
            .0
            .dyn_into::<Function>()
            .map_err(error::Export::GetSplice)?;
        Reflect::apply(&method, &o, &args).map_err(error::Export::CallSplice)?;
        Ok(o.into())
    }
}

pub(crate) fn alloc(value: &Value<'_>) -> (Datatype, JsValue) {
    match value {
        am::Value::Object(o) => match o {
            ObjType::Map => (Datatype::Map, Object::new().into()),
            ObjType::Table => (Datatype::Table, Object::new().into()),
            ObjType::List => (Datatype::List, Array::new().into()),
            ObjType::Text => (Datatype::Text, Array::new().into()),
        },
        am::Value::Scalar(s) => match s.as_ref() {
            am::ScalarValue::Bytes(v) => (Datatype::Bytes, Uint8Array::from(v.as_slice()).into()),
            am::ScalarValue::Str(v) => (Datatype::Str, v.to_string().into()),
            am::ScalarValue::Int(v) => (Datatype::Int, (*v as f64).into()),
            am::ScalarValue::Uint(v) => (Datatype::Uint, (*v as f64).into()),
            am::ScalarValue::F64(v) => (Datatype::F64, (*v).into()),
            am::ScalarValue::Counter(v) => (Datatype::Counter, (f64::from(v)).into()),
            am::ScalarValue::Timestamp(v) => (
                Datatype::Timestamp,
                js_sys::Date::new(&(*v as f64).into()).into(),
            ),
            am::ScalarValue::Boolean(v) => (Datatype::Boolean, (*v).into()),
            am::ScalarValue::Null => (Datatype::Null, JsValue::null()),
            am::ScalarValue::Unknown { bytes, type_code } => (
                Datatype::Unknown(*type_code),
                Uint8Array::from(bytes.as_slice()).into(),
            ),
        },
    }
}

fn set_hidden_value<V: Into<JsValue>>(
    o: &Object,
    key: &Symbol,
    value: V,
) -> Result<(), error::Export> {
    let definition = Object::new();
    js_set(&definition, "value", &value.into()).map_err(|_| error::Export::SetHidden("value"))?;
    js_set(&definition, "writable", false).map_err(|_| error::Export::SetHidden("writable"))?;
    js_set(&definition, "enumerable", false).map_err(|_| error::Export::SetHidden("enumerable"))?;
    js_set(&definition, "configurable", false)
        .map_err(|_| error::Export::SetHidden("configurable"))?;
    Object::define_property(o, &key.into(), &definition);
    Ok(())
}

fn shallow_copy(obj: &Object) -> Object {
    if Array::is_array(obj) {
        Array::from(obj).into()
    } else {
        Object::assign(&Object::new(), obj)
    }
}

fn prop_to_js(prop: &Prop) -> JsValue {
    match prop {
        Prop::Map(key) => key.into(),
        Prop::Seq(index) => (*index as f64).into(),
    }
}

pub(crate) mod error {
    use automerge::LoadChangeError;
    use wasm_bindgen::JsValue;

    #[derive(Debug, thiserror::Error)]
    pub enum BadJSChanges {
        #[error("the changes were not an array of Uint8Array")]
        ChangesNotArray,
        #[error("change {0} was not a Uint8Array")]
        ElemNotUint8Array(usize),
        #[error("error loading change {0}: {1}")]
        BadChange(usize, LoadChangeError),
    }

    #[derive(Debug, thiserror::Error)]
    pub enum BadChangeHashes {
        #[error("the change hashes were not an array of strings")]
        NotArray,
        #[error("could not decode hash {0}: {1}")]
        BadElem(usize, BadChangeHash),
    }

    impl From<BadChangeHashes> for JsValue {
        fn from(e: BadChangeHashes) -> Self {
            JsValue::from(e.to_string())
        }
    }

    #[derive(Debug, thiserror::Error)]
    pub enum BadChangeHashSet {
        #[error("not an object")]
        NotObject,
        #[error(transparent)]
        GetProp(#[from] GetProp),
        #[error("unable to getOwnProperties")]
        ListProp,
        #[error("unable to parse hash from {0:?}: {1}")]
        BadHash(wasm_bindgen::JsValue, BadChangeHash),
    }

    #[derive(Debug, thiserror::Error)]
    pub enum BadChangeHash {
        #[error("change hash was not a string")]
        NotString,
        #[error(transparent)]
        Parse(#[from] automerge::ParseChangeHashError),
    }

    impl From<BadChangeHash> for JsValue {
        fn from(e: BadChangeHash) -> Self {
            JsValue::from(e.to_string())
        }
    }

    #[derive(Debug, thiserror::Error)]
    pub enum BadSyncState {
        #[error(transparent)]
        GetProp(#[from] GetProp),
        #[error("bad sharedHeads: {0}")]
        BadSharedHeads(BadChangeHashes),
        #[error("bad lastSentHeads: {0}")]
        BadLastSentHeads(BadChangeHashes),
        #[error("bad theirHeads: {0}")]
        BadTheirHeads(BadChangeHashes),
        #[error("bad theirNeed: {0}")]
        BadTheirNeed(BadChangeHashes),
        #[error("bad theirHave: {0}")]
        BadTheirHave(BadHaves),
        #[error("bad sentHashes: {0}")]
        BadSentHashes(BadChangeHashSet),
        #[error("inFlight not a boolean")]
        InFlightNotBoolean,
    }

    impl From<BadSyncState> for JsValue {
        fn from(e: BadSyncState) -> Self {
            JsValue::from(e.to_string())
        }
    }

    #[derive(Debug, thiserror::Error)]
    #[error("unable to get property {property}: {error:?}")]
    pub struct GetProp {
        pub(super) property: String,
        pub(super) error: wasm_bindgen::JsValue,
    }

    impl From<GetProp> for JsValue {
        fn from(e: GetProp) -> Self {
            JsValue::from(e.to_string())
        }
    }

    #[derive(Debug, thiserror::Error)]
    #[error("error setting property {property:?} on JS value: {error:?}")]
    pub struct SetProp {
        pub(super) property: JsValue,
        pub(super) error: JsValue,
    }

    impl From<SetProp> for JsValue {
        fn from(e: SetProp) -> Self {
            JsValue::from(e.to_string())
        }
    }

    #[derive(Debug, thiserror::Error)]
    pub enum BadHave {
        #[error("bad lastSync: {0}")]
        BadLastSync(BadChangeHashes),
        #[error("bad bloom: {0}")]
        BadBloom(BadBloom),
        #[error(transparent)]
        GetHaveProp(#[from] GetProp),
    }

    #[derive(Debug, thiserror::Error)]
    pub enum BadHaves {
        #[error("value was not an array")]
        NotArray,
        #[error("error loading have at index {0}: {1}")]
        BadElem(usize, BadHave),
    }

    #[derive(Debug, thiserror::Error)]
    pub enum BadBloom {
        #[error("the value was not a Uint8Array")]
        NotU8Array,
        #[error("unable to decode: {0}")]
        Decode(#[from] automerge::sync::DecodeBloomError),
    }

    #[derive(Debug, thiserror::Error)]
    pub enum Export {
        #[error(transparent)]
        Set(#[from] SetProp),
        #[error("unable to delete prop {prop}: {err:?}")]
        Delete { prop: String, err: JsValue },
        #[error("unable to set hidden property {0}")]
        SetHidden(&'static str),
        #[error("data handler for type {0} did not return a valid object")]
        InvalidDataHandler(String),
        #[error("error calling data handler for type {0}: {1:?}")]
        CallDataHandler(String, JsValue),
        #[error(transparent)]
        GetProp(#[from] GetProp),
        #[error(transparent)]
        InvalidDatatype(#[from] crate::value::InvalidDatatype),
        #[error("unable to get the splice function: {0:?}")]
        GetSplice(JsValue),
        #[error("error calling splice: {0:?}")]
        CallSplice(JsValue),
    }

    impl From<Export> for JsValue {
        fn from(e: Export) -> Self {
            JsValue::from(e.to_string())
        }
    }

    #[derive(Debug, thiserror::Error)]
    pub enum ApplyPatch {
        #[error(transparent)]
        Export(#[from] Export),
        #[error("cannot delete from a seq")]
        DeleteKeyFromSeq,
        #[error("cannot put key in seq")]
        PutKeyInSeq,
        #[error("cannot increment a non-numeric value")]
        IncrementNonNumeric,
        #[error("cannot increment a key in a seq")]
        IncrementKeyInSeq,
        #[error("cannot increment index in a map")]
        IncrementIndexInMap,
        #[error("cannot insert into a map")]
        InsertInMap,
        #[error("cannot splice into a map")]
        SpliceInMap,
        #[error("cannot put a seq index in a map")]
        PutIdxInMap,
        #[error(transparent)]
        GetProp(#[from] GetProp),
        #[error(transparent)]
        SetProp(#[from] SetProp),
    }

    impl From<ApplyPatch> for JsValue {
        fn from(e: ApplyPatch) -> Self {
            JsValue::from(e.to_string())
        }
    }

    #[derive(Debug, thiserror::Error)]
    pub enum BadSyncMessage {
        #[error(transparent)]
        GetProp(#[from] GetProp),
        #[error("unable to read haves: {0}")]
        BadHaves(#[from] BadHaves),
        #[error("could not read changes: {0}")]
        BadJSChanges(#[from] BadJSChanges),
        #[error("could not read heads: {0}")]
        BadHeads(BadChangeHashes),
        #[error("could not read need: {0}")]
        BadNeed(BadChangeHashes),
    }

    impl From<BadSyncMessage> for JsValue {
        fn from(e: BadSyncMessage) -> Self {
            JsValue::from(e.to_string())
        }
    }
}