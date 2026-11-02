// syntax.js — our own syntax highlighting: a state-based tokenizer with JS regular
// expressions. Tokenizing goes line by line; the state at the end of a line (inside a
// comment or a multi-line string) is the initial state of the next one.
//
// A language: { id, name, exts, names?, comment ('//'), block (['/*','*/']), states }.
// states: { root: [[regex, type, next?], ...], ... }. 'next' is another state,
// or '@pop' to go back to root. The types are those of colorFor() (keyword, string, comment…).

const kw = (words) => new RegExp(`\\b(?:${words.split(' ').join('|')})\\b`, 'y');
const R = (src, flags = '') => new RegExp(src, `y${flags}`);

const NUM = R('(?:0[xX][0-9a-fA-F_]+|0[bB][01_]+|0[oO][0-7_]+|(?:\\d[\\d_]*\\.?[\\d_]*|\\.\\d[\\d_]*)(?:[eE][+-]?\\d+)?)[a-zA-Z]*');
const IDENT_CALL = R('[A-Za-z_$][\\w$]*(?=\\s*\\()');
const TYPE_CAP = R('[A-Z][\\w$]*');
const IDENT = R('[A-Za-z_$][\\w$]*');
const OP = R('[+\\-*/%=&|^!<>~?:]+');
const PUNCT = R('[()[\\]{}.,;@]');
const SPACE = R('\\s+');

// Quoted strings (with escapes), on a single line.
const str = (q) => R(`${q}(?:[^${q}\\\\]|\\\\.)*${q}?`);

function cLike({ keywords, types = '', constants = 'true false null', extra = [], hashComment = false, templates = false }) {
  return {
    root: [
      [SPACE, 'text'],
      ...(hashComment ? [[R('#.*'), 'comment']] : [[R('//.*'), 'comment']]),
      [R('/\\*'), 'comment', 'block'],
      ...extra,
      ...(templates ? [[R('`'), 'string', 'template']] : []),
      [str('"'), 'string'],
      [str("'"), 'string'],
      [NUM, 'number'],
      [kw(constants), 'constant'],
      [kw(keywords), 'keyword'],
      ...(types ? [[kw(types), 'type']] : []),
      [IDENT_CALL, 'function'],
      [TYPE_CAP, 'type'],
      [IDENT, 'text'],
      [OP, 'operator'],
      [PUNCT, 'punct'],
    ],
    block: [[R('[^*]+'), 'comment'], [R('\\*/'), 'comment', '@pop'], [R('\\*'), 'comment']],
    template: [[R('[^`\\\\$]+'), 'string'], [R('\\\\.'), 'string'], [R('\\$\\{[^}]*\\}'), 'variable'], [R('\\$'), 'string'], [R('`'), 'string', '@pop']],
  };
}

const JS_KW = 'break case catch class const continue debugger default delete do else export extends finally for from function if import in instanceof let new of return static super switch this throw try typeof var void while with yield async await get set as interface type enum implements private public protected readonly declare namespace abstract keyof';

export const LANGUAGES = [
  { id: 'plain', name: 'Texto', exts: ['txt', 'log', ''], comment: '', states: { root: [[R('.+'), 'text']] } },
  { id: 'js', name: 'JavaScript', exts: ['js', 'mjs', 'cjs', 'jsx', 'ts', 'tsx', 'mts'], comment: '//', block: ['/*', '*/'],
    states: cLike({ keywords: JS_KW, types: 'string number boolean any unknown never object symbol bigint', constants: 'true false null undefined NaN Infinity',
      templates: true, extra: [[R('/(?![/*])(?:[^/\\\\\\n]|\\\\.)+/[gimsuyd]*(?=\\s*[.,;)\\]}]|$)'), 'regexp'], [R('</?[A-Za-z][\\w.]*'), 'tag']] }) },
  { id: 'c', name: 'C / C++', exts: ['c', 'h', 'cc', 'cpp', 'cxx', 'hpp', 'hh', 'ino'], comment: '//', block: ['/*', '*/'],
    states: cLike({ keywords: 'auto break case catch class const constexpr continue default delete do else enum explicit extern for friend goto if inline namespace new noexcept operator private protected public register restrict return sizeof static static_assert struct switch template this throw try typedef typename union using virtual volatile while',
      types: 'void char short int long float double signed unsigned bool size_t ssize_t uint8_t uint16_t uint32_t uint64_t int8_t int16_t int32_t int64_t FILE',
      constants: 'true false NULL nullptr', extra: [[R('#\\s*\\w+.*'), 'meta']] }) },
  { id: 'go', name: 'Go', exts: ['go'], comment: '//', block: ['/*', '*/'],
    states: cLike({ keywords: 'break case chan const continue default defer else fallthrough for func go goto if import interface map package range return select struct switch type var',
      types: 'bool byte complex64 complex128 error float32 float64 int int8 int16 int32 int64 rune string uint uint8 uint16 uint32 uint64 uintptr any', constants: 'true false nil iota',
      extra: [[R('`[^`]*`?'), 'string']] }) },
  { id: 'rust', name: 'Rust', exts: ['rs'], comment: '//', block: ['/*', '*/'],
    states: cLike({ keywords: 'as async await break const continue crate dyn else enum extern fn for if impl in let loop match mod move mut pub ref return self Self static struct super trait type unsafe use where while',
      types: 'i8 i16 i32 i64 i128 isize u8 u16 u32 u64 u128 usize f32 f64 bool char str String Vec Option Result Box', constants: 'true false None Some Ok Err',
      extra: [[R('[a-z_]+!'), 'function'], [R("'[a-z_]+\\b(?!')"), 'type']] }) },
  { id: 'python', name: 'Python', exts: ['py', 'pyw', 'pyi'], names: [], comment: '#',
    states: {
      root: [
        [SPACE, 'text'], [R('#.*'), 'comment'],
        [R('[rRbBuUfF]{0,2}"""'), 'string', 'tdq'], [R("[rRbBuUfF]{0,2}'''"), 'string', 'tsq'],
        [R('[rRbBuUfF]{0,2}' + str('"').source), 'string'], [R('[rRbBuUfF]{0,2}' + str("'").source), 'string'],
        [R('@[\\w.]+'), 'meta'], [NUM, 'number'], [kw('True False None'), 'constant'],
        [kw('and as assert async await break class continue def del elif else except finally for from global if import in is lambda nonlocal not or pass raise return try while with yield match case self'), 'keyword'],
        [kw('int float str bool list dict set tuple bytes object type'), 'type'],
        [IDENT_CALL, 'function'], [TYPE_CAP, 'type'], [IDENT, 'text'], [OP, 'operator'], [PUNCT, 'punct'],
      ],
      tdq: [[R('(?:[^"\\\\]|\\\\.|"(?!""))+'), 'string'], [R('"""'), 'string', '@pop']],
      tsq: [[R("(?:[^'\\\\]|\\\\.|'(?!''))+"), 'string'], [R("'''"), 'string', '@pop']],
    } },
  { id: 'shell', name: 'Shell', exts: ['sh', 'bash', 'zsh', 'fish', 'bashrc', 'zshrc', 'profile'], names: ['Makefile', 'makefile', 'Dockerfile'], comment: '#',
    states: { root: [
      [SPACE, 'text'], [R('#.*'), 'comment'], [str('"'), 'string'], [str("'"), 'string'],
      [R('\\$\\{[^}]*\\}|\\$[\\w@#?*!$-]+|\\$\\('), 'variable'], [NUM, 'number'],
      [kw('if then else elif fi for while until do done case esac in function return local export readonly declare set unset shift exit source alias break continue select time'), 'keyword'],
      [R('[\\w.-]+(?==)'), 'variable'], [R('-{1,2}[\\w-]+'), 'constant'], [IDENT, 'text'], [R('[|&;<>()]+'), 'operator'], [R('.'), 'text'],
    ] } },
  { id: 'json', name: 'JSON', exts: ['json', 'jsonc', 'json5', 'webmanifest'], comment: '//',
    states: { root: [[SPACE, 'text'], [R('"(?:[^"\\\\]|\\\\.)*"(?=\\s*:)'), 'attr'], [str('"'), 'string'], [NUM, 'number'], [kw('true false null'), 'constant'], [R('//.*'), 'comment'], [PUNCT, 'punct'], [R('.'), 'text']] } },
  { id: 'markdown', name: 'Markdown', exts: ['md', 'markdown', 'mdx'], comment: '',
    states: {
      root: [
        [R('#{1,6}\\s.*'), 'heading'], [R('```.*'), 'meta', 'code'], [R('\\s*(?:[-*+]|\\d+\\.)\\s'), 'keyword'], [R('>.*'), 'comment'],
        [R('\\*\\*[^*]+\\*\\*|__[^_]+__'), 'bold'], [R('\\*[^*\\s][^*]*\\*|_[^_\\s][^_]*_'), 'italic'], [R('`[^`]+`'), 'string'],
        [R('!?\\[[^\\]]*\\]\\([^)]*\\)'), 'link'], [R('[^#`*_\\[!>]+'), 'text'], [R('.'), 'text'],
      ],
      code: [[R('```\\s*$'), 'meta', '@pop'], [R('.+'), 'string']],
    } },
  { id: 'html', name: 'HTML', exts: ['html', 'htm', 'xml', 'svg', 'xhtml', 'vue'], comment: '', block: ['<!--', '-->'],
    states: {
      root: [[R('<!--'), 'comment', 'comment'], [R('<!\\w[^>]*>'), 'meta'], [R('</?[\\w:-]+'), 'tag', 'tag'], [R('&\\w+;'), 'constant'], [R('[^<&]+'), 'text'], [R('.'), 'text']],
      tag: [[SPACE, 'text'], [R('/?>'), 'tag', '@pop'], [R('[\\w:@.-]+(?==)'), 'attr'], [R('='), 'operator'], [str('"'), 'string'], [str("'"), 'string'], [R('[\\w:-]+'), 'attr'], [R('.'), 'text']],
      comment: [[R('(?:[^-]|-(?!->))+'), 'comment'], [R('-->'), 'comment', '@pop'], [R('-'), 'comment']],
    } },
  { id: 'css', name: 'CSS', exts: ['css', 'scss', 'less'], comment: '', block: ['/*', '*/'],
    states: {
      root: [[SPACE, 'text'], [R('/\\*'), 'comment', 'block'], [R('//.*'), 'comment'], [R('@[\\w-]+'), 'keyword'], [R('[.#][\\w-]+'), 'type'], [R('[\\w-]+(?=\\s*:[^;{]*[;}])'), 'attr'],
        [R('#[0-9a-fA-F]{3,8}\\b'), 'number'], [NUM, 'number'], [str('"'), 'string'], [str("'"), 'string'], [R('[\\w-]+(?=\\()'), 'function'], [R('[\\w-]+'), 'text'], [R('[{}:;,()>+~*]'), 'punct'], [R('.'), 'text']],
      block: [[R('[^*]+'), 'comment'], [R('\\*/'), 'comment', '@pop'], [R('\\*'), 'comment']],
    } },
  { id: 'yaml', name: 'YAML', exts: ['yaml', 'yml'], comment: '#',
    states: { root: [[SPACE, 'text'], [R('#.*'), 'comment'], [R('-{3}|\\.{3}'), 'meta'], [R('-(?=\\s)'), 'punct'], [R('[\\w.-]+(?=\\s*:(?:\\s|$))'), 'attr'], [str('"'), 'string'], [str("'"), 'string'],
      [kw('true false null yes no on off'), 'constant'], [NUM, 'number'], [R('[&*][\\w-]+'), 'variable'], [R('[:|>]'), 'operator'], [R('[^\\s#]+'), 'string']] } },
  { id: 'php', name: 'PHP', exts: ['php', 'phtml'], comment: '//', block: ['/*', '*/'],
    states: cLike({ keywords: 'abstract and array as break callable case catch class clone const continue declare default do echo else elseif empty enddeclare endfor endforeach endif endswitch endwhile extends final finally fn for foreach function global goto if implements include include_once instanceof insteadof interface isset list match namespace new or print private protected public require require_once return static switch throw trait try unset use var while xor yield',
      constants: 'true false null TRUE FALSE NULL', extra: [[R('\\$\\w+'), 'variable'], [R('<\\?php|\\?>'), 'meta'], [R('#.*'), 'comment']] }) },
  { id: 'sql', name: 'SQL', exts: ['sql'], comment: '--', block: ['/*', '*/'],
    states: { root: [[SPACE, 'text'], [R('--.*'), 'comment'], [R('/\\*'), 'comment', 'block'], [str("'"), 'string'], [str('"'), 'attr'], [NUM, 'number'],
      [kw('select from where and or not insert into values update set delete create table drop alter add column index primary key foreign references join left right inner outer full on group by order having limit offset as distinct union all exists in is null like between case when then else end begin commit rollback view trigger default unique check returning with', 'i'), 'keyword'],
      [R('\\b(?:SELECT|FROM|WHERE|AND|OR|NOT|INSERT|INTO|VALUES|UPDATE|SET|DELETE|CREATE|TABLE|DROP|ALTER|ADD|COLUMN|INDEX|PRIMARY|KEY|FOREIGN|REFERENCES|JOIN|LEFT|RIGHT|INNER|OUTER|ON|GROUP|BY|ORDER|HAVING|LIMIT|OFFSET|AS|DISTINCT|UNION|ALL|EXISTS|IN|IS|NULL|LIKE|BETWEEN|CASE|WHEN|THEN|ELSE|END|WITH|RETURNING)\\b'), 'keyword'],
      [kw('int integer bigint smallint text varchar char boolean date timestamp numeric decimal real serial json jsonb uuid blob'), 'type'], [IDENT_CALL, 'function'], [IDENT, 'text'], [OP, 'operator'], [PUNCT, 'punct']],
      block: [[R('[^*]+'), 'comment'], [R('\\*/'), 'comment', '@pop'], [R('\\*'), 'comment']] } },
  { id: 'ini', name: 'INI / TOML', exts: ['ini', 'toml', 'conf', 'cfg', 'desktop', 'service', 'properties', 'env'], names: ['.gitconfig', '.editorconfig'], comment: '#',
    states: { root: [[SPACE, 'text'], [R('[#;].*'), 'comment'], [R('\\[\\[?[^\\]]*\\]\\]?'), 'heading'], [R('[\\w.\\[\\]@-]+(?=\\s*=)'), 'attr'], [R('='), 'operator'], [str('"'), 'string'], [str("'"), 'string'],
      [kw('true false yes no on off'), 'constant'], [NUM, 'number'], [R('.+'), 'string']] } },
  { id: 'diff', name: 'Diff', exts: ['diff', 'patch'], comment: '',
    states: { root: [[R('(?:\\+\\+\\+|---) .*'), 'meta'], [R('@@.*'), 'function'], [R('\\+.*'), 'inserted'], [R('-.*'), 'deleted'], [R('(?:diff|index) .*'), 'meta'], [R('.*'), 'text']] } },
];

const byId = new Map(LANGUAGES.map((l) => [l.id, l]));
export const languageById = (id) => byId.get(id) || byId.get('plain');

export function languageFor(name) {
  const base = String(name || '').split('/').pop();
  for (const l of LANGUAGES) if (l.names?.includes(base)) return l;
  const m = /\.([^.]+)$/.exec(base);
  const ext = m ? m[1].toLowerCase() : base.startsWith('.') ? base.slice(1).toLowerCase() : '';
  for (const l of LANGUAGES) if (l.exts.includes(ext) && ext) return l;
  return byId.get('plain');
}

// Tokenizes a line from a state. Returns { tokens: [[text, type], ...], state }.
// States are stacked ('a/b/c') for nested ones.
export function tokenizeLine(lang, line, state = 'root') {
  const tokens = [];
  let stack = state.split('/');
  let i = 0;
  const push = (text, type) => {
    const last = tokens[tokens.length - 1];
    if (last && last[1] === type) last[0] += text; else tokens.push([text, type]);
  };
  let guard = 0;
  while (i < line.length && guard++ < 10000) {
    const rules = lang.states[stack[stack.length - 1]] || lang.states.root;
    let matched = false;
    for (const [re, type, next] of rules) {
      re.lastIndex = i;
      const m = re.exec(line);
      if (!m || m[0].length === 0) continue;
      push(m[0], type);
      i += m[0].length;
      if (next === '@pop') { if (stack.length > 1) stack.pop(); } else if (next) stack.push(next);
      matched = true;
      break;
    }
    if (!matched) { push(line[i], stack.length > 1 ? (rules[0]?.[1] || 'text') : 'text'); i++; }
  }
  // a line comment or a single-line string does not continue on the next line
  return { tokens, state: stack.join('/') };
}

// TextMate / classic IDE-style palette, dark and light.
const DARK = {
  text: '#e4e4e7', comment: '#7f8c98', string: '#fc6a5d', keyword: '#fc5fa3', number: '#d0bf69', constant: '#d0bf69',
  type: '#5dd8ff', function: '#67b7a4', operator: '#e4e4e7', punct: '#a1a1aa', regexp: '#ff8170', variable: '#a167e6',
  meta: '#fd8f3f', tag: '#5dd8ff', attr: '#bf8555', heading: '#5dd8ff', bold: '#ffffff', italic: '#d4d4d8', link: '#6bb3ff',
  inserted: '#7ee787', deleted: '#ff7b72',
};
const LIGHT = {
  text: '#1f1f24', comment: '#5d6c79', string: '#c41a16', keyword: '#9b2393', number: '#1c00cf', constant: '#1c00cf',
  type: '#0b4f79', function: '#326d74', operator: '#1f1f24', punct: '#5f5f6b', regexp: '#d12f1b', variable: '#6c36a9',
  meta: '#643820', tag: '#0b4f79', attr: '#815f03', heading: '#0b4f79', bold: '#000000', italic: '#3f3f46', link: '#0e64c8',
  inserted: '#116329', deleted: '#a40e26',
};
export const paletteFor = (mode) => (mode === 'light' ? LIGHT : DARK);
