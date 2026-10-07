'use strict';
// Beginner "simple object" facade (Phase WD2-C "First Object").
//
// PURE and browser-safe: requires only sibling src/vrml modules. No fs, no
// Electron, no CodeMirror, no DOM.
//
// A VIEW over the CURRENT parse, never a model. A beginner thinks "this Box has
// a Position, a Rotation, a Size and a Color"; VRML97 spreads those over a
// Transform, a geometry node and a Material. This module only answers
//
//   * is this node the Transform of a recognisable simple object, and which
//     primitive does it hold?
//   * which existing AST node supplies Position / Rotation / Size|Radius /
//     Color, and which schema field is it?
//
// It owns no value, no selection and no identity: every value it reports is
// read from the parse through field-edit.js inspectNodeFields (authored) or the
// WD1.3 schema default (absent), and every change it plans is an ordinary
// field-edit.js planFieldEdit (authored field) or structure-edit.js
// planFieldInsert (absent field) on that same node. It does not parse, and it
// keeps no state between calls.
//
// Recognised shape (everything else -> null, and the UI shows the real node
// type instead of a friendly name):
//
//   Transform {                       <- the object; Position, Rotation
//     children [ Shape { ... } ]      <- exactly ONE child, a Shape node
//   }                                    (an un-bracketed single value is fine)
//   Shape {
//     geometry Box|Sphere { ... }     <- authored once, a node (not USE)
//     appearance Appearance {         <- optional; Color needs it
//       material Material { ... }     <- optional; Color needs it (not USE)
//     }
//   }

const { NODE } = require('./ast');
const tx = require('./document-transaction');
const fieldEdit = require('./field-edit');
const structureEdit = require('./structure-edit');
const schema = require('./node-schema');

// Beginner label -> the role node + schema field that supplies it. One table;
// the schema stays the authority on each field's type, default and range.
const PROPERTY_MAP = Object.freeze({
  Box: Object.freeze([
    Object.freeze({ key: 'position', label: 'Position', role: 'transform', field: 'translation' }),
    Object.freeze({ key: 'rotation', label: 'Rotation', role: 'transform', field: 'rotation' }),
    Object.freeze({ key: 'size', label: 'Size', role: 'geometry', field: 'size' }),
    Object.freeze({ key: 'color', label: 'Color', role: 'material', field: 'diffuseColor' }),
  ]),
  Sphere: Object.freeze([
    Object.freeze({ key: 'position', label: 'Position', role: 'transform', field: 'translation' }),
    Object.freeze({ key: 'rotation', label: 'Rotation', role: 'transform', field: 'rotation' }),
    Object.freeze({ key: 'radius', label: 'Radius', role: 'geometry', field: 'radius' }),
    Object.freeze({ key: 'color', label: 'Color', role: 'material', field: 'diffuseColor' }),
  ]),
});

// Plain-language component labels for the beginner controls. The technical
// labels (X/Y/Z, R/G/B, X/Y/Z/Angle) stay in the WD2-B Inspector.
const COMPONENT_LABELS = Object.freeze({
  position: Object.freeze(['X', 'Y', 'Z']),
  rotation: Object.freeze(['Axis X', 'Axis Y', 'Axis Z', 'Angle (radians)']),
  size: Object.freeze(['Width (X)', 'Height (Y)', 'Depth (Z)']),
  radius: Object.freeze(['Radius']),
  color: Object.freeze(['Red', 'Green', 'Blue']),
});

const OBJECT_REASON = Object.freeze({
  NOT_A_SIMPLE_OBJECT: 'not-a-simple-object',
  UNKNOWN_PROPERTY: 'unknown-property',
  NO_MATERIAL: 'object-has-no-material',
  ROLE_NOT_A_NODE: 'property-node-not-editable',
});

const fieldsNamed = (node, name) => (Array.isArray(node.fields) ? node.fields : [])
  .filter((f) => f && f.type === NODE.FIELD && f.name === name);

// The single node value of a field authored exactly once, else null. A USE is
// not a node instance and is never returned.
function singleNodeValue(node, name) {
  const fs = fieldsNamed(node, name);
  if (fs.length !== 1 || fs[0].isBinding) return null;
  const v = fs[0].value;
  return v && v.type === NODE.NODE ? v : null;
}

/**
 * Recognise a simple object rooted at `node` in the session's parse.
 *
 * @returns {{primitive:string, transform:object, shape:object, geometry:object,
 *   material:object|null}|null} Frozen, or null when the structure is not the
 *   exact recognised shape.
 */
function recognize(node) {
  if (!node || node.type !== NODE.NODE || node.nodeType !== 'Transform') return null;
  const children = fieldsNamed(node, 'children');
  if (children.length !== 1 || children[0].isBinding) return null;
  const v = children[0].value;
  let shape = null;
  if (v && v.type === NODE.ARRAY) {
    const items = Array.isArray(v.items) ? v.items : [];
    if (items.length !== 1) return null;
    shape = items[0];
  } else {
    shape = v;
  }
  if (!shape || shape.type !== NODE.NODE || shape.nodeType !== 'Shape') return null;
  const geometry = singleNodeValue(shape, 'geometry');
  if (!geometry || !Object.prototype.hasOwnProperty.call(PROPERTY_MAP, geometry.nodeType)) return null;
  const appearance = singleNodeValue(shape, 'appearance');
  const material = appearance && appearance.nodeType === 'Appearance' ? singleNodeValue(appearance, 'material') : null;
  return Object.freeze({
    primitive: geometry.nodeType,
    transform: node,
    shape,
    geometry,
    material: material && material.nodeType === 'Material' ? material : null,
  });
}

const roleNode = (obj, role) => (role === 'transform' ? obj.transform : role === 'geometry' ? obj.geometry : obj.material);

/**
 * The friendly display label for a node ("Box" / "Sphere"), or null. Display
 * only: never written to the source, never used for identity.
 */
function displayLabel(node) {
  const obj = recognize(node);
  return obj ? obj.primitive : null;
}

/**
 * The beginner properties of the simple object rooted at `node`.
 *
 * Each property reports the current value from the parse: the authored tokens
 * when the field is present (field-edit inspectNodeFields, so the beginner panel
 * and the Inspector can never disagree), else the schema default. `editable`
 * and `reason` come from the same gates the edit itself will run.
 *
 * @returns {{primitive:string, properties:object[]}|{reason:string}} Frozen.
 */
function describeObject(session, node, options = {}) {
  if (!tx.isParseSession(session)) return Object.freeze({ reason: fieldEdit.FIELD_EDIT_REASON.STALE_SESSION });
  const obj = recognize(node);
  if (!obj) return Object.freeze({ reason: OBJECT_REASON.NOT_A_SIMPLE_OBJECT });
  const currentText = options.currentText;
  const properties = PROPERTY_MAP[obj.primitive].map((p) => {
    const target = roleNode(obj, p.role);
    const record = schema.getFieldSchema(target ? target.nodeType : (p.role === 'material' ? 'Material' : ''), p.field);
    const base = {
      key: p.key,
      label: p.label,
      field: p.field,
      nodeType: target ? target.nodeType : 'Material',
      type: record ? record.type : null,
      componentLabels: COMPONENT_LABELS[p.key],
      kind: p.key === 'color' ? 'color' : 'number',
    };
    if (!target) {
      return Object.freeze({ ...base, present: false, editable: false, reason: OBJECT_REASON.NO_MATERIAL, values: Object.freeze([]) });
    }
    const described = fieldEdit.inspectNodeFields(session, target, { currentText });
    const authored = described.fields.filter((f) => f.name === p.field);
    if (authored.length === 0) {
      return Object.freeze({
        ...base,
        present: false,
        editable: described.status === fieldEdit.FIELD_EDIT_STATUS.EDITABLE,
        reason: described.reason,
        // The schema's own default spelling (e.g. "0 0 1 0"), shown as the value.
        values: Object.freeze(String(record.defaultText).trim().split(/\s+/)),
      });
    }
    const f = authored[0];
    return Object.freeze({
      ...base,
      present: true,
      editable: authored.length === 1 && f.editable,
      reason: authored.length === 1 ? f.reason : fieldEdit.FIELD_EDIT_REASON.FIELD_DUPLICATED,
      values: Object.freeze(f.components.map((c) => String(c.text))),
    });
  });
  return Object.freeze({ primitive: obj.primitive, properties: Object.freeze(properties) });
}

/**
 * Plan one beginner property change. An authored field is patched in place by
 * field-edit.js planFieldEdit (component tokens only); an absent field is
 * inserted by structure-edit.js planFieldInsert. Same plan shape either way.
 */
function planPropertySet(request) {
  const { session, currentText, node, key, components } = request || {};
  const refuse = (reason) => Object.freeze({ status: fieldEdit.PLAN_STATUS.REFUSED, reason });
  if (!tx.isParseSession(session)) return refuse(fieldEdit.FIELD_EDIT_REASON.STALE_SESSION);
  const obj = recognize(node);
  if (!obj) return refuse(OBJECT_REASON.NOT_A_SIMPLE_OBJECT);
  const p = PROPERTY_MAP[obj.primitive].find((x) => x.key === key);
  if (!p) return refuse(OBJECT_REASON.UNKNOWN_PROPERTY);
  const target = roleNode(obj, p.role);
  if (!target) return refuse(OBJECT_REASON.NO_MATERIAL);
  const list = Array.isArray(target.fields) ? target.fields : [];
  const indexes = [];
  list.forEach((f, i) => { if (f && f.type === NODE.FIELD && f.name === p.field) indexes.push(i); });
  if (indexes.length > 1) return refuse(fieldEdit.FIELD_EDIT_REASON.FIELD_DUPLICATED);
  const plan = indexes.length === 1
    ? fieldEdit.planFieldEdit({ session, currentText, node: target, fieldIndex: indexes[0], fieldName: p.field, components })
    : structureEdit.planFieldInsert({ session, currentText, node: target, fieldName: p.field, components });
  return plan.status === fieldEdit.PLAN_STATUS.READY ? Object.freeze({ ...plan, property: key }) : plan;
}

module.exports = {
  OBJECT_REASON,
  PROPERTY_MAP,
  recognize,
  displayLabel,
  describeObject,
  planPropertySet,
};
