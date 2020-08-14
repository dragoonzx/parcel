use neon::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;

mod graph;
use graph::{Graph, Value};

fn js_value_to_value(cx: &mut CallContext<JsGraph>, js: &Handle<JsValue>) -> NeonResult<Value> {
  match js.downcast::<JsArray>() {
    Ok(array) => {
      let js_value_vec = array.to_vec(cx)?;
      let mut value_vec = Vec::new();
      for js_value in js_value_vec {
        value_vec.push(js_value_to_value(cx, &js_value)?)
      }
      return Ok(Value::Array(value_vec));
    }
    Err(_) => {}
  };
  match js.downcast::<JsObject>() {
    Ok(object) => {
      let mut obj_map: HashMap<String, Value> = HashMap::new();
      for name in object.get_own_property_names(cx)?.to_vec(cx)? {
        let name_str = name.downcast::<JsString>().or_throw(cx)?.value();
        let value = object.get(cx, &name_str[..])?;
        obj_map.insert(name_str.clone(), js_value_to_value(cx, &value)?);
      }
      return Ok(Value::Object(obj_map));
    }
    Err(_) => {}
  };
  match js.downcast::<JsNumber>() {
    Ok(num) => return Ok(Value::F64(num.value())),
    Err(_) => {}
  };
  match js.downcast::<JsBoolean>() {
    Ok(boolean) => return Ok(Value::Bool(boolean.value())),
    Err(_) => {}
  };
  match js.downcast::<JsString>() {
    Ok(string) => return Ok(Value::String(string.value())),
    Err(_) => {}
  };
  match js.downcast::<JsNull>() {
    Ok(_) => return Ok(Value::Null),
    Err(_) => {}
  };
  match js.downcast::<JsUndefined>() {
    Ok(_) => return Ok(Value::Undefined),
    Err(_) => {}
  };
  unreachable!();
}

fn value_to_js_value<'a>(
  cx: &mut CallContext<'a, JsGraph>,
  value: &Value,
) -> JsResult<'a, JsValue> {
  Ok(match value {
    Value::F64(num) => cx.number(num.clone()).upcast(),
    Value::String(string) => cx.string(string).upcast(),
    Value::Null => cx.null().upcast(),
    Value::Undefined => cx.undefined().upcast(),
    Value::Bool(boolean) => cx.boolean(boolean.clone()).upcast(),
    Value::Array(vector) => {
      // Adapted from https://neon-bindings.com/docs/arrays
      let js_array = JsArray::new(cx, vector.len() as u32);
      for (i, value) in vector.iter().enumerate() {
        let js_value = value_to_js_value(cx, value)?;
        js_array.set(cx, i as u32, js_value)?;
      }
      js_array.upcast()
    }
    Value::Object(obj_map) => {
      let obj = JsObject::new(cx);
      for (key, value) in obj_map {
        let js_value = value_to_js_value(cx, value)?;
        obj.set(cx, &key[..], js_value)?;
      }
      obj.upcast()
    }
  })
}

declare_types! {
  pub class JsGraph for Graph {
    init(mut _cx) {
      Ok(Graph::new())
    }

    method addNode(mut cx) {
      let mut this = cx.this();
      let js_value = cx.argument::<JsObject>(0)?;
      let value = match js_value_to_value(&mut cx, &js_value.upcast())? {
        Value::Object(obj_map) => {
          obj_map
        },
        _ => unreachable!()
      };

      let guard = cx.lock();
      let mut graph = this.borrow_mut(&guard);
      match graph.add_node(&value) {
        Ok(()) => Ok(js_value.upcast()),
        Err(err) => panic!(err),
      }
    }

    method getNode(mut cx) {
      let id = cx.argument::<JsString>(0)?.value();
      let mut this = cx.this();

      let value: Option<Value>;
      {
        let guard = cx.lock();
        let mut graph = this.borrow_mut(&guard);
        let weight = graph.get_node(&id[..]);
        value = match weight {
          Some(w) => {
            Some(Value::Object(w.clone()))
          },
          None => None
        }
      };

      match value {
        Some(v) => value_to_js_value(&mut cx, &v),
        None => Ok(cx.undefined().upcast())
      }
    }

    method setRootNode(mut cx) {
      let mut this = cx.this();
      let js_value = cx.argument::<JsObject>(0)?;
      let value = match js_value_to_value(&mut cx, &js_value.upcast())? {
        Value::Object(obj_map) => {
          obj_map
        },
        _ => {
          return cx.throw_error("Node is not an object")
        }
      };

      {
        let guard = cx.lock();
        let mut graph = this.borrow_mut(&guard);
        let _ = graph.set_root_node(&value);
      };

      Ok(cx.undefined().upcast())
    }

    method addEdge(mut cx) {
      let mut this = cx.this();
      let id_a = cx.argument::<JsString>(0)?.value();
      let id_b = cx.argument::<JsString>(1)?.value();
      let edge_type = match cx.argument_opt(2) {
        Some(edge_type) => {
          Some(edge_type.downcast::<JsString>().or_throw(&mut cx)?.value())
        },
        None => None,
      };

      {
        let guard = cx.lock();
        let mut graph = this.borrow_mut(&guard);
        match edge_type {
          Some(edge_type) => {
            let _ = graph.add_edge(&id_a[..], &id_b[..], Some(&edge_type[..]));
          },
          None => {
            let _ = graph.add_edge(&id_a[..], &id_b[..], None);
          }
        }
      };
      Ok(cx.undefined().upcast())
    }

    method removeNode(mut cx) {
      let mut this = cx.this();
      let js_value = cx.argument::<JsObject>(0)?;
      let value = match js_value_to_value(&mut cx, &js_value.upcast())? {
        Value::Object(obj_map) => {
          obj_map
        },
        _ => {
          return cx.throw_error("Node is not an object")
        }
      };

      let removed = {
        let guard = cx.lock();
        let mut graph = this.borrow_mut(&guard);

        graph.remove_node(&value)
      };

      match removed {
        Some(_) => Ok(cx.undefined().upcast()),
        None => return cx.throw_error("Does not have node")
      }
    }

    method removeById(mut cx) {
      let mut this = cx.this();
      let id = cx.argument::<JsString>(0)?.value();

      {
        let guard = cx.lock();
        let mut graph = this.borrow_mut(&guard);

        let _ = graph.remove_by_id(&id[..]);
      };

      Ok(cx.undefined().upcast())
    }

    method traverse(mut cx) {
      let mut this = cx.this();
      let cb = cx.argument::<JsFunction>(0)?;
      let start_node = cx.argument_opt(1).and_then(|start_node| {
        if let Ok(_) = start_node.downcast::<JsNull>() {
          return None
        } else if let Ok(_) = start_node.downcast::<JsUndefined>() {
          return None
        }

        let js_object = start_node.downcast::<JsObject>().or_throw(&mut cx).unwrap();
        let converted_value = match js_value_to_value(&mut cx, &js_object.upcast()).unwrap() {
          Value::Object(obj_map) => obj_map,
          _ => unimplemented!(),
        };


        Some(converted_value)
      });

      let edge_type = cx.argument_opt(1).and_then(|edge_type| {
        if let Ok(_) = edge_type.downcast::<JsNull>() {
          return None
        } else if let Ok(_) = edge_type.downcast::<JsUndefined>() {
          return None
        }

        Some(edge_type.downcast::<JsString>().or_throw(&mut cx).unwrap().value())
      });

      let undefined = cx.undefined();
      {
        let guard = cx.lock();
        let mut graph = this.borrow_mut(&guard);

        let _ = graph.traverse(start_node.as_ref(), edge_type.as_ref().map(|e| &e[..]), move |node| {
          cb.call(&mut cx, cx.null(), value_to_js_value(&mut cx, &Value::Object(node.clone())));
        });
      };

      Ok(undefined.upcast())
    }
  }
}

register_module!(mut cx, {
  cx.export_class::<JsGraph>("Graph")?;
  Ok(())
});