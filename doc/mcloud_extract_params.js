const fs = require('fs');
const path = require('path');
const infile = '/tmp/mcloud_api_results.json';
if (!fs.existsSync(infile)) { console.error('input not found'); process.exit(2); }
const arr = JSON.parse(fs.readFileSync(infile,'utf8'));
const byPath = new Map();
for (const e of arr) {
  const key = e.request || '(variable_request)';
  if (!byPath.has(key)) byPath.set(key, []);
  byPath.get(key).push(e);
}

function findFunctionBody(text, funcName, approxIndex) {
  if (!funcName) return null;
  // find occurrence of function header near approxIndex
  const re = new RegExp('(?:async\\s+)?' + funcName.replace(/[-\\/\\^$*+?.()|[\]{}]/g,'\\$&') + '\\s*\\([^)]*\\)\\s*\\{', 'g');
  let match; let best = null; while ((match = re.exec(text))!==null) {
    const idx = match.index;
    // prefer match that is before approxIndex and closest
    if (approxIndex && idx > approxIndex + 200) break; // too far ahead
    best = {idx, match};
    if (approxIndex && idx > approxIndex - 2000) break; // close enough
  }
  if (!best) return null;
  const start = text.indexOf('{', best.idx);
  if (start < 0) return null;
  // find matching brace
  let i = start; let depth = 0; let inStr = null; let esc=false;
  for (let j=start;j<text.length;j++){
    const ch = text[j];
    if (inStr) {
      if (esc) esc=false; else if (ch==='\\\\') esc=true; else if (ch===inStr) inStr=null;
      continue;
    } else {
      if (ch==='"' || ch==="'" || ch==='`') { inStr = ch; continue; }
    }
    if (ch==='{') depth++;
    else if (ch==='}') { depth--; if (depth===0) { return text.slice(start+1, j); } }
  }
  return null;
}

function extractObjectLiteral(body, varName='data') {
  // search for patterns like 'let data = {' or 'data = {'
  const re = new RegExp('(?:let|var|const)?\\s*' + varName + '\\s*=\\s*\\{','g');
  let match; while ((match = re.exec(body))!==null) {
    const s = match.index + match[0].length - 1;
    // find matching brace
    let depth = 0; let inStr=null; let esc=false;
    for (let j=s;j<body.length;j++){
      const ch = body[j];
      if (inStr) {
        if (esc) esc=false; else if (ch==='\\\\') esc=true; else if (ch===inStr) inStr=null;
        continue;
      } else {
        if (ch==='"' || ch==="'" || ch==='`') { inStr = ch; continue; }
      }
      if (ch==='{') depth++;
      else if (ch==='}') { depth--; if (depth===0) { return body.slice(match.index + match[0].length -1, j+1); } }
    }
  }
  // also check inline in Utils.sendRequest call: Utils.sendRequest('/path', { ... }
  const callRe = /Utils\.sendRequest\s*\(\s*(['"`])([^'"`]+)\1\s*,\s*\{/g;
  let c;
  while ((c = callRe.exec(body))!==null) {
    const s = c.index + c[0].length - 1;
    let depth=0; let inStr=null; let esc=false;
    for (let j=s;j<body.length;j++){
      const ch = body[j];
      if (inStr) {
        if (esc) esc=false; else if (ch==='\\\\') esc=true; else if (ch===inStr) inStr=null;
        continue;
      } else {
        if (ch==='"' || ch==="'" || ch==='`') { inStr = ch; continue; }
      }
      if (ch==='{') depth++;
      else if (ch==='}') { depth--; if (depth===0) { return body.slice(s, j+1); } }
    }
  }
  return null;
}

const out = [];
for (const [key, items] of byPath.entries()) {
  const sample = items[0];
  const file = sample.file;
  let body = null;
  try {
    const text = fs.readFileSync(file,'utf8');
    body = findFunctionBody(text, sample.funcName, sample.index) || null;
    if (!body) {
      // fallback: extract surrounding 1000 chars around match index
      const s = Math.max(0, sample.index - 1000);
      const e = Math.min(text.length, sample.index + 1000);
      body = text.slice(s,e);
    }
    const dataLiteral = extractObjectLiteral(body,'data') || extractObjectLiteral(body,'params') || extractObjectLiteral(body,'root');
    out.push({path: key, functions: [...new Set(items.map(i=>i.funcName).filter(Boolean))], files: [...new Set(items.map(i=>i.file))].slice(0,3).map(f=>f.replace('/tmp/mcloud_asar_extract/','')), methodGuess: [...new Set(items.map(i=>i.methodGuess))], sampleLine: sample.line, dataLiteral: dataLiteral? dataLiteral.trim().slice(0,2000): null, note: dataLiteral? 'literal found' : 'no literal; may build data dynamically'});
  } catch (e) {
    out.push({path:key, error: e.message});
  }
}
fs.writeFileSync('/tmp/mcloud_api_detailed.json', JSON.stringify(out,null,2));
// generate markdown
const md = [];
md.push('# Detailed API document (with parameter snippets, best-effort)');
md.push('Note: object literals extracted heuristically from function bodies.');
for (const item of out) {
  md.push(`## ${item.path}`);
  md.push(`- Functions: ${item.functions.join(', ')}`);
  md.push(`- Files: ${item.files.join(', ')}`);
  md.push(`- Method (guessed): ${item.methodGuess.join(', ')}`);
  md.push(`- Sample line: ${item.sampleLine}`);
  if (item.dataLiteral) {
    md.push('\n```javascript\n' + item.dataLiteral + '\n```\n');
  } else if (item.error) {
    md.push('- Error reading file: ' + item.error);
  } else {
    md.push('- 参数示例未在函数中以静态对象形式发现；可能动态构造。');
  }
}
fs.writeFileSync('/tmp/mcloud_api_doc_detailed.md', md.join('\n\n'));
console.log('WROTE /tmp/mcloud_api_doc_detailed.md and /tmp/mcloud_api_detailed.json');
