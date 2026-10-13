'use strict';
// New-node source fragments (Phase WD2-C "First Object").
//
// PURE and browser-safe: requires only the WD1.3 node schema. No fs, no
// Electron, no CodeMirror, no DOM, no parser.
//
// THIS IS NOT A SERIALIZER. It never takes an AST, a node, a field value or any
// existing document text and prints it back. It only produces the text of a
// BRAND NEW node the user asked to create, from a fixed template, so it can be
// inserted at a provable offset by src/vrml/structure-edit.js. Existing source
// is never regenerated (WD.md section 2).
//
// Generated objects are ordinary VRML97:
//
//   * ANONYMOUS -- no `DEF`. Editor identity comes from the verified insertion
//     transaction (structure-edit.js resolveInsertedNode), never from a name the
//     editor invents and writes into the user's file.
//   * no editor metadata -- no WorldInfo, no comment markers, no proprietary
//     field, nothing a reader would not have written by hand;
//   * no default-valued fields -- `Transform.translation 0 0 0`, `Box.size
//     2 2 2` and friends are left absent; changing one later INSERTS exactly that
//     field (structure-edit.js planFieldInsert);
//   * the semantic shape is Transform -> Shape -> Appearance -> Material +
//     primitive geometry, because Position/Rotation need a Transform and Color
//     needs a Material.
//
// Deterministic: the same arguments always give the same text. The caller
// passes the document's own line ending so a CRLF document gets CRLF lines.

const schema = require('./node-schema');

// The canonical VRML97 header line (ISO/IEC 14772-1 5.2.1), without a line end.
const VRML97_HEADER = '#VRML V2.0 utf8';

// The primitives WD2-C exposes. Adding one later is one entry here plus its
// beginner property mapping in src/vrml/simple-object.js.
const PRIMITIVES = Object.freeze(['Box', 'Sphere']);

// The fixed node chain every template uses, checked against the schema at load
// so a typo can never emit a non-VRML97 node.
const TEMPLATE_NODES = Object.freeze(['Transform', 'Shape', 'Appearance', 'Material']);
const TEMPLATE_FIELDS = Object.freeze([
  ['Transform', 'children'], ['Shape', 'appearance'], ['Shape', 'geometry'], ['Appearance', 'material'],
]);

const TEMPLATE_ERROR = Object.freeze({
  PRIMITIVE: 'ETEMPLATEPRIMITIVE',
  EOL: 'ETEMPLATEEOL',
});

function templateError(code, message) {
  const err = new Error(message);
  err.code = code;
  return err;
}

for (const n of TEMPLATE_NODES.concat(PRIMITIVES)) {
  if (!schema.isVRML97Node(n)) throw new Error(`node-templates: ${n} is not a VRML97 node in the schema`);
}
for (const [n, f] of TEMPLATE_FIELDS) {
  if (!schema.isFieldAllowed(n, f, 'vrml97')) throw new Error(`node-templates: ${n}.${f} is not a VRML97 field`);
}

const EOLS = new Set(['\n', '\r\n']);

/**
 * The text of a new anonymous simple object: a Transform holding one Shape with
 * an Appearance/Material and the requested primitive geometry.
 *
 * The first line has no indentation; nested lines use two spaces per level. The
 * text neither starts nor ends with a line ending -- placement (and the
 * separators around it) is structure-edit.js's job.
 *
 * @param {string} primitive One of PRIMITIVES.
 * @param {{eol?: '\n'|'\r\n'}} [options]
 * @returns {{text:string, nodeType:'Transform', primitive:string}} Frozen.
 * @throws {Error} ETEMPLATEPRIMITIVE / ETEMPLATEEOL for programming errors.
 */
function simpleObjectTemplate(primitive, options = {}) {
  if (!PRIMITIVES.includes(primitive)) {
    throw templateError(TEMPLATE_ERROR.PRIMITIVE, `simpleObjectTemplate: unsupported primitive ${String(primitive)}`);
  }
  const eol = options.eol === undefined ? '\n' : options.eol;
  if (!EOLS.has(eol)) throw templateError(TEMPLATE_ERROR.EOL, 'simpleObjectTemplate: eol must be "\\n" or "\\r\\n"');
  const lines = [
    'Transform {',
    '  children [',
    '    Shape {',
    '      appearance Appearance {',
    '        material Material {',
    '        }',
    '      }',
    `      geometry ${primitive} {`,
    '      }',
    '    }',
    '  ]',
    '}',
  ];
  return Object.freeze({ text: lines.join(eol), nodeType: 'Transform', primitive });
}

module.exports = {
  VRML97_HEADER,
  PRIMITIVES,
  TEMPLATE_ERROR,
  simpleObjectTemplate,
};
