const fs = require('fs');
const path = require('path');

function walk(dir) {
  let results = [];
  const list = fs.readdirSync(dir, { withFileTypes: true });
  for (const d of list) {
    const p = path.join(dir, d.name);
    if (d.isDirectory()) results = results.concat(walk(p));
    else if (/\.js$/.test(d.name)) results.push(p);
  }
  return results;
}

function extractFromText(text, file) {
  const results = [];
  const sendRegex = /Utils\.sendRequest\s*\(/g;
  let match;
  while ((match = sendRegex.exec(text)) !== null) {
    const startIdx = match.index + match[0].length - 1; // position at (
    // find matching closing parenthesis
    let i = startIdx;
    let depth = 0;
    let inStr = null;
    let escaped = false;
    let parenContent = '';
    for (let j = i; j < text.length; j++) {
      const ch = text[j];
      parenContent += ch;
      if (inStr) {
        if (escaped) { escaped = false; }
        else if (ch === '\\') escaped = true;
        else if (ch === inStr) inStr = null;
        continue;
      } else {
        if (ch === '"' || ch === "'" || ch === '`') { inStr = ch; continue; }
      }
      if (ch === '(') depth++;
      else if (ch === ')') {
        depth--;
        if (depth === 0) break;
      }
    }
    // remove outer paren
    if (parenContent[0] === '(') parenContent = parenContent.slice(1, -1);
    // try to get first arg string literal
    let firstArg = null;
    const m = parenContent.match(/^\s*(['"`])([^'"`]+)\1/);
    if (m) firstArg = m[2];
    // if firstArg is null, try find requestUrl variable above
    if (!firstArg) {
      // search backwards up to 2000 chars for requestUrl = '...'
      const before = text.slice(Math.max(0, match.index - 2000), match.index);
      const mr = before.match(/requestUrl\s*=\s*(['"`])([^'"`]+)\1/);
      if (mr) firstArg = mr[2];
    }
    // guess method: count top-level commas
    // simple parser to count top-level commas
    let commas = 0;
    let level = 0;
    inStr = null; escaped = false;
    for (let k = 0; k < parenContent.length; k++) {
      const ch = parenContent[k];
      if (inStr) {
        if (escaped) { escaped = false; }
        else if (ch === '\\') escaped = true;
        else if (ch === inStr) inStr = null;
        continue;
      } else {
        if (ch === '"' || ch === "'" || ch === '`') { inStr = ch; continue; }
      }
      if (ch === '(' || ch === '{' || ch === '[') level++;
      else if (ch === ')' || ch === '}' || ch === ']') level--;
      else if (ch === ',' && level === 0) commas++;
    }
    let methodGuess = (commas >= 1) ? 'POST (data present)' : 'GET';
    // override if options contains method:get
    if (/method\s*[:=]\s*['"`]get['"`]/i.test(parenContent)) methodGuess = 'GET (forced)';

    // find function name and category by scanning backwards by lines
    const lines = text.split(/\n/);
    // compute line number of match
    const prefix = text.slice(0, match.index);
    const lineNum = prefix.split(/\n/).length; // 1-based
    let funcName = null;
    let categories = [];
    for (let l = lineNum - 1; l >= Math.max(0, lineNum - 400); l--) {
      const line = lines[l];
      if (!funcName) {
        let m1 = line.match(/^[\s\t\r]*(?:async\s+)?([A-Za-z0-9_]+)\s*\([^)]*\)\s*\{\s*$/);
        if (m1) { funcName = m1[1]; /*console.log('fn',funcName)*/; break; }
        let m2 = line.match(/^[\s\t\r]*([A-Za-z0-9_]+)\s*:\s*function\s*\(/);
        if (m2) { funcName = m2[1]; break; }
        let m3 = line.match(/^[\s\t\r]*([A-Za-z0-9_]+)\s*:\s*async\s*function\s*\(/);
        if (m3) { funcName = m3[1]; break; }
        // sometimes method defined as 'getX: async (params) => {' or 'getX: (params) => {'
        let m4 = line.match(/^[\s\t\r]*([A-Za-z0-9_]+)\s*:\s*(?:async\s*)?\([^)]*\)\s*=>\s*\{\s*$/);
        if (m4) { funcName = m4[1]; break; }
        // or 'name(params) {' without async
        let m5 = line.match(/^[\s\t\r]*([A-Za-z0-9_]+)\s*\([^)]*\)\s*\{\s*$/);
        if (m5) { funcName = m5[1]; break; }
      }
    }
    // find categories (nearest "key: {" above)
    for (let l = lineNum - 1; l >= Math.max(0, lineNum - 800); l--) {
      const line = lines[l];
      let mc = line.match(/^[\s\t\r]*([A-Za-z0-9_]+)\s*:\s*\{\s*$/);
      if (mc) {
        categories.push(mc[1]);
        if (categories.length >= 3) break;
      }
      if (/export\s+default\s*\{/.test(line)) break;
    }
    // reverse categories to get outer->inner
    categories = categories.reverse();

    results.push({file, index: match.index, line: lineNum, funcName, categories, request: firstArg, methodGuess, argsSnippet: parenContent.trim().slice(0,500)});
  }
  return results;
}

const baseDirs = [
  '/tmp/mcloud_asar_extract/out/renderer',
  '/tmp/mcloud_asar_extract/out/main'
].filter(d=>fs.existsSync(d));
let allFiles = [];
for (const d of baseDirs) allFiles = allFiles.concat(walk(d));
let all = [];
for (const f of allFiles) {
  try {
    const txt = fs.readFileSync(f,'utf8');
    const found = extractFromText(txt, f);
    if (found.length) all = all.concat(found);
  } catch (e) {
    // ignore
  }
}
fs.writeFileSync('/tmp/mcloud_api_results.json', JSON.stringify(all,null,2));
console.log('WROTE /tmp/mcloud_api_results.json with', all.length, 'entries');
