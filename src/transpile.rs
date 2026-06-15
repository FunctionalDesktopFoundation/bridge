use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::Path;

thread_local! {
    static COMPONENT_MAP: RefCell<HashMap<String, (String, Vec<String>)>> = RefCell::new(HashMap::new());
}

fn convert_inline_expr(expr: &str) -> String {
    let trimmed = expr.trim_start();
    if trimmed.starts_with("function") {
        return expr.to_string();
    }
    convert_pythonic_expr(expr)
}

thread_local! {
    static INLINE_ID_CTR: std::cell::Cell<u32> = std::cell::Cell::new(0);
    static INLINE_COMPONENTS: std::cell::RefCell<Vec<String>> = std::cell::RefCell::new(Vec::new());
}

fn is_jsx_start(s: &str) -> bool {
    let trimmed = s.trim_start();
    trimmed.starts_with('<') && trimmed.len() > 1
        && (trimmed.as_bytes()[1].is_ascii_alphabetic() || trimmed.as_bytes()[1] == b'_' || trimmed.as_bytes()[1] == b'.')
}

fn inline_jsx_to_component(value: &str) -> Result<String, String> {
    let (qml, consumed) = jsx_to_qml_inner(value, 0)?;
    let id = INLINE_ID_CTR.with(|ctr| {
        let n = ctr.get();
        ctr.set(n + 1);
        format!("_inline_{}", n)
    });
    let comp = format!("Component {{ id: {}\n{}}}", id, qml);
    INLINE_COMPONENTS.with(|vc| vc.borrow_mut().push(comp));
    Ok(id)
}

fn get_inline_components() -> String {
    INLINE_COMPONENTS.with(|vc| {
        let mut result = String::new();
        for c in vc.borrow().iter() {
            result.push_str(c);
            result.push('\n');
        }
        result
    })
}

fn clear_inline_components() {
    INLINE_ID_CTR.with(|ctr| ctr.set(0));
    INLINE_COMPONENTS.with(|vc| vc.borrow_mut().clear());
}

pub fn transpile(source: &str, base: &str) -> Result<String, String> {
    COMPONENT_MAP.with(|cm| cm.borrow_mut().clear());
    clear_inline_components();
    let mut s = source.to_string();
    remove_comments(&mut s);
    process_requires(&mut s, base)?;
    let imports = extract_imports(&mut s);
    convert_pythonic_exprs(&mut s);
    s = convert_pythonic_blocks(&s)?;

    let mut cleaned = String::new();
    for line in s.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("app ") && trimmed.ends_with(':') {
            continue;
        }
        cleaned.push_str(line);
        cleaned.push('\n');
    }
    s = cleaned;

    s = convert_component_blocks_inline(&s)?;
    let converted = convert_all_jsx_to_qml(&s)?;

    let mut result = String::new();
    for imp in imports.lines() {
        let t = imp.trim();
        if !t.is_empty() {
            result.push_str(t);
            result.push('\n');
        }
    }

    let inline_comps = get_inline_components();
    if !inline_comps.is_empty() {
        for line in inline_comps.lines() {
            let t = line.trim();
            if !t.is_empty() {
                result.push_str(t);
                result.push('\n');
            }
        }
        result.push('\n');
    }

    result.push_str(&converted);

    result = fix_root_on_completed(&result);
    result = nest_components_inside_window(&result);
    result = clean_output(&result);
    Ok(promote_imports(&result))
}

fn remove_comments(s: &mut String) {
    let mut r = String::new();
    for line in s.lines() {
        r.push_str(&strip_python_comment(line));
        r.push('\n');
    }
    *s = r;
}

fn strip_python_comment(line: &str) -> String {
    let mut result = String::new();
    let mut in_string = false;
    let mut in_double = false;
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if !in_string && !in_double {
            if chars[i] == '"' {
                in_string = true;
            } else if chars[i] == '\'' {
                in_double = true;
            } else if chars[i] == '#' {
                break;
            }
        } else if in_string {
            if chars[i] == '"' {
                in_string = false;
            } else if chars[i] == '\\' && i + 1 < chars.len() {
                result.push(chars[i]);
                i += 1;
            }
        } else if in_double {
            if chars[i] == '\'' {
                in_double = false;
            } else if chars[i] == '\\' && i + 1 < chars.len() {
                result.push(chars[i]);
                i += 1;
            }
        }
        result.push(chars[i]);
        i += 1;
    }
    result
}

fn process_requires(s: &mut String, base: &str) -> Result<(), String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut iterations = 0;
    let mut current_pos = 0;

    loop {
        iterations += 1;
        if iterations > 2000 {
            return Err("Infinite loop fallback triggered inside require statements processing. Please check cyclical dependencies.".to_string());
        }

        if let Some(offset) = s[current_pos..].find("require \"") {
            let start = current_pos + offset;
            let after = &s[start + 9..];
            if let Some(end) = after.find('"') {
                let path = after[..end].to_string();
                let full = Path::new(base).join(&path);
                let canonical = full.to_string_lossy().to_string();

                if seen.contains(&canonical) {
                    let before = s[..start].to_string();
                    let after_part = s[start + 9 + end + 1..].to_string();
                    *s = format!("{}{}", before, after_part);
                    continue;
                }
                seen.insert(canonical);

                let content = fs::read_to_string(&full)
                    .map_err(|e| format!("require '{}' failed: {}", path, e))?;
                let mut cleaned = String::new();
                for cl in content.lines() {
                    cleaned.push_str(&strip_python_comment(cl));
                    cleaned.push('\n');
                }
                let wrapped = format!("/*@qml\n{}@qml*/\n", cleaned);
                let before = s[..start].to_string();
                let after_part = s[start + 9 + end + 1..].to_string();
                *s = format!("{}{}{}", before, wrapped, after_part);
                current_pos = start + wrapped.len();
            } else {
                break;
            }
        } else {
            break;
        }
    }
    Ok(())
}

fn extract_imports(s: &mut String) -> String {
    let mut im = Vec::new();
    let mut rest = String::new();
    for line in s.lines() {
        let t = line.trim_start();
        if t.starts_with("import ") {
            im.push(t.to_string());
        } else {
            rest.push_str(line);
            rest.push('\n');
        }
    }
    *s = rest;
    im.join("\n")
}

fn convert_pythonic_exprs(s: &mut String) {
    let reps = [
        (" is not ", " !== "),
        (" is ", " === "),
        (" and ", " && "),
        (" or ", " || "),
        (" not ", " ! "),
        ("None", "null"),
        ("True", "true"),
        ("False", "false"),
    ];
    for (f, t) in &reps {
        *s = s.replace(f, t);
    }
    *s = s.replace("{not ", "{! ");
    *s = s.replace("(not ", "(! ");
    *s = s.replace("{and ", "{&& ");
    *s = s.replace("(and ", "(&& ");
    *s = s.replace("{or ", "{|| ");
    *s = s.replace("(or ", "(|| ");
    *s = s.replace("{is ", "{=== ");
    *s = s.replace("(is ", "(=== ");
}

fn convert_pythonic_expr(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.starts_with("function") || trimmed.starts_with("function(") {
        return value.to_string();
    }

    let v = convert_ternary(value);
    let padded = format!(" {} ", v);
    let mut v = padded;
    let reps = [
        (" is not ", " !== "),
        (" is ", " === "),
        (" and ", " && "),
        (" or ", " || "),
        (" not ", " ! "),
        ("None", "null"),
        ("True", "true"),
        ("False", "false"),
    ];
    for (f, t) in &reps {
        v = v.replace(f, t);
    }
    v[1..v.len() - 1].to_string()
}

fn convert_ternary(expr: &str) -> String {
    if let Some(if_word_pos) = find_ternary_if(expr) {
        let before_if = expr[..if_word_pos].trim_end();
        let rest = &expr[if_word_pos + 3..];
        if let Some(else_pos) = find_ternary_else(rest) {
            let cond = rest[..else_pos].trim();
            let after_else = rest[else_pos + 5..].trim();
            if !cond.ends_with(':') {
                if let Some(eq_pos) = before_if.rfind('=') {
                    if eq_pos == 0
                        || !"!<>="
                            .contains(before_if.chars().nth(eq_pos - 1).unwrap_or(' '))
                    {
                        let lhs = before_if[..eq_pos].trim();
                        let true_val = before_if[eq_pos + 1..].trim();
                        return format!("{} = {} ? {} : {}", lhs, cond, true_val, after_else);
                    }
                }
                return format!("{} ? {} : {}", cond, before_if, after_else);
            }
        }
    }
    expr.to_string()
}

fn find_ternary_if(s: &str) -> Option<usize> {
    let chars: Vec<char> = s.chars().collect();
    let mut in_string = false;
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '"' {
            in_string = !in_string;
            i += 1;
            continue;
        }
        if in_string {
            i += 1;
            continue;
        }
        if chars[i] == ' '
            && i + 3 < chars.len()
            && chars[i + 1] == 'i'
            && chars[i + 2] == 'f'
            && chars[i + 3] == ' '
        {
            let before = &s[..i];
            if !before.trim().is_empty() {
                return Some(i + 1);
            }
        }
        i += 1;
    }
    None
}

fn find_ternary_else(s: &str) -> Option<usize> {
    let chars: Vec<char> = s.chars().collect();
    let mut in_string = false;
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '"' {
            in_string = !in_string;
            i += 1;
            continue;
        }
        if in_string {
            i += 1;
            continue;
        }
        if chars[i] == ' '
            && i + 5 < chars.len()
            && chars[i + 1] == 'e'
            && chars[i + 2] == 'l'
            && chars[i + 3] == 's'
            && chars[i + 4] == 'e'
            && chars[i + 5] == ' '
        {
            return Some(i + 1);
        }
        i += 1;
    }
    None
}

fn convert_pythonic_blocks(source: &str) -> Result<String, String> {
    let lines: Vec<&str> = source.lines().collect();
    let mut output = String::new();
    let mut i = 0;
    let mut in_raw_qml = false;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        if !in_raw_qml && trimmed == "/*@qml" {
            in_raw_qml = true;
            output.push_str(line);
            output.push('\n');
            i += 1;
            continue;
        }

        if in_raw_qml {
            output.push_str(line);
            output.push('\n');
            i += 1;
            if trimmed == "@qml*/" {
                in_raw_qml = false;
            }
            continue;
        }

        if is_def_line(trimmed) {
            let (js, consumed) = transpile_def_block(&lines[i..])?;
            output.push_str(&js);
            output.push('\n');
            i += consumed;
        } else {
            output.push_str(line);
            output.push('\n');
            i += 1;
        }
    }

    Ok(output)
}

fn is_def_line(line: &str) -> bool {
    if let Some(rest) = line.strip_prefix("def ") {
        let paren = rest.find('(');
        let colon = rest.rfind(':');
        match (paren, colon) {
            (Some(p), Some(c)) if p < c => {
                let name = rest[..p].trim();
                !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_')
            }
            _ => false,
        }
    } else {
        false
    }
}

fn transpile_def_block(lines: &[&str]) -> Result<(String, usize), String> {
    let first = lines[0].trim();
    let after_def = first.strip_prefix("def ").unwrap();
    let paren = after_def.find('(').ok_or("Invalid def: missing (")?;
    let name = after_def[..paren].trim();
    let colon = after_def.rfind(':').ok_or("Invalid def: missing :")?;
    let args = after_def[paren + 1..colon].trim();
    let args_cleaned = args.trim_end_matches(')');

    let base_indent = count_indent(lines[0]);
    let mut body_lines: Vec<&str> = Vec::new();
    let mut consumed = 1;

    for &l in lines.iter().skip(1) {
        if l.trim().is_empty() {
            body_lines.push(l);
            consumed += 1;
            continue;
        }
        let indent = count_indent(l);
        if indent <= base_indent {
            break;
        }
        body_lines.push(l);
        consumed += 1;
    }

    let body_base = body_lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| count_indent(l))
        .min()
        .unwrap_or(base_indent + 4);

    let body_js = parse_pythonic_block(&body_lines, body_base)?;
    let js = format!(
        "function {}({}) {{\n{}\n}}",
        name,
        args_cleaned,
        body_js.trim()
    );
    Ok((js, consumed))
}

fn parse_pythonic_block(lines: &[&str], base_indent: usize) -> Result<String, String> {
    let mut output = String::new();
    let total = lines.len();
    let mut i = 0;

    while i < total {
        let line = lines[i];
        let trimmed = line.trim();

        if trimmed.is_empty() {
            output.push('\n');
            i += 1;
            continue;
        }

        let indent = count_indent(line);
        if indent < base_indent {
            break;
        }

        if is_def_line(trimmed) {
            let (js, _) = transpile_def_block(&lines[i..])?;
            for js_line in js.lines() {
                output.push_str(&" ".repeat(base_indent));
                output.push_str(js_line);
                output.push('\n');
            }
            let mut skip = 1;
            for &l in lines.iter().skip(i + 1) {
                if l.trim().is_empty() {
                    skip += 1;
                    continue;
                }
                if count_indent(l) <= indent {
                    break;
                }
                skip += 1;
            }
            i += skip;
            continue;
        }

        if indent != base_indent {
            output.push_str(&" ".repeat(indent));
            output.push_str(&convert_pythonic_expr(trimmed));
            output.push_str(";\n");
            i += 1;
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("if ") {
            let is_pythonic = rest.trim().ends_with(':');
            if !is_pythonic {
                let rt = rest.trim();
                if let Some(colon) = rt.find(':') {
                    let before = &rt[..colon];
                    if bracket_depth(before) == 0 && !before.trim().is_empty() {
                        let after = rt[colon + 1..].trim();
                        if !after.is_empty() {
                            output.push_str(&" ".repeat(indent));
                            output.push_str(&format!(
                                "if ({}) {};\n",
                                convert_pythonic_expr(before),
                                convert_pythonic_expr(after)
                            ));
                            i += 1;
                            continue;
                        }
                    }
                }
                output.push_str(&" ".repeat(indent));
                output.push_str(trimmed);
                output.push_str(";\n");
                i += 1;
                continue;
            }

            let cond = rest.trim_end_matches(':').trim();
            output.push_str(&format!("if ({}) {{\n", convert_pythonic_expr(cond)));
            let (b, n) = collect_and_parse_body(&lines[i + 1..], indent)?;
            output.push_str(&b);
            i += 1 + n;

            while i < total {
                let pl = lines[i];
                let pt = pl.trim();
                let pi = count_indent(pl);
                if pi == indent && pt.starts_with("elif ") {
                    let cond = pt.strip_prefix("elif ").unwrap().trim_end_matches(':').trim();
                    output.push_str(&format!("}} else if ({}) {{\n", convert_pythonic_expr(cond)));
                    let (b2, n2) = collect_and_parse_body(&lines[i + 1..], indent)?;
                    output.push_str(&b2);
                    i += 1 + n2;
                } else if pi == indent && (pt == "else:" || pt.starts_with("else ")) {
                    let is_pythonic_else = pt == "else:" || pt.ends_with(':');
                    if !is_pythonic_else {
                        output.push_str(&" ".repeat(indent));
                        output.push_str(pt);
                        output.push_str(";\n");
                        i += 1;
                        break;
                    }
                    output.push_str("} else {\n");
                    let (b3, n3) = collect_and_parse_body(&lines[i + 1..], indent)?;
                    output.push_str(&b3);
                    i += 1 + n3;
                    break;
                } else {
                    break;
                }
            }
            output.push_str("}\n");
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("for ") {
            let rt = rest.trim();
            if let Some(colon) = rt.find(':') {
                let before = &rt[..colon];
                if bracket_depth(before) == 0 && !before.trim().is_empty() {
                    let after = rt[colon + 1..].trim();
                    if !after.is_empty() {
                        let for_parts: Vec<&str> = before.splitn(2, " in ").collect();
                        if for_parts.len() == 2 {
                            let var_name = for_parts[0].trim();
                            let iter_expr = for_parts[1].trim();
                            let stmt = convert_pythonic_expr(after);
                            output.push_str(&" ".repeat(indent));
                            if let Some(range_args) = iter_expr.strip_prefix("range(") {
                                let ra = range_args.trim_end_matches(')').trim();
                                if let Some(comma) = ra.find(',') {
                                    let start = ra[..comma].trim();
                                    let end = ra[comma + 1..].trim();
                                    output.push_str(&format!("for (var {} = {}; {} < {}; {}++) {};\n", var_name, start, var_name, end, var_name, stmt));
                                } else {
                                    output.push_str(&format!("for (var {} = 0; {} < {}; {}++) {};\n", var_name, var_name, ra, var_name, stmt));
                                }
                            } else {
                                let tmp = format!("_i{}", total.saturating_sub(i));
                                let iter_js = convert_pythonic_expr(iter_expr);
                                output.push_str(&format!("for (var {} = 0; {} < {}.length; {}++) {{ var {} = {}[{}]; {}; }}\n", tmp, tmp, iter_js, tmp, var_name, iter_js, tmp, stmt));
                            }
                            i += 1;
                            continue;
                        }
                    }
                }
            }
            let parts: Vec<&str> = rest.trim_end_matches(':').splitn(2, " in ").collect();
            if parts.len() == 2 {
                let var_name = parts[0].trim();
                let iter_expr = parts[1].trim();
                if let Some(range_args) = iter_expr.strip_prefix("range(") {
                    let ra = range_args.trim_end_matches(')').trim();
                    if let Some(comma) = ra.find(',') {
                        let start = ra[..comma].trim();
                        let end = ra[comma + 1..].trim();
                        output.push_str(&format!("for (var {} = {}; {} < {}; {}++) {{\n", var_name, start, var_name, end, var_name));
                    } else {
                        output.push_str(&format!("for (var {} = 0; {} < {}; {}++) {{\n", var_name, var_name, ra, var_name));
                    }
                } else {
                    let tmp = format!("_i{}", total.saturating_sub(i));
                    let iter_js = convert_pythonic_expr(iter_expr);
                    output.push_str(&format!(
                        "for (var {} = 0; {} < {}.length; {}++) {{\n",
                        tmp, tmp, iter_js, tmp
                    ));
                    output.push_str(&format!("    var {} = {}[{}];\n", var_name, iter_js, tmp));
                }
                let (b, n) = collect_and_parse_body(&lines[i + 1..], indent)?;
                output.push_str(&b);
                output.push_str("}\n");
                i += 1 + n;
                continue;
            } else {
                return Err(format!("Invalid for syntax: {}", trimmed));
            }
        }

        if let Some(rest) = trimmed.strip_prefix("while ") {
            let cond = rest.trim_end_matches(':').trim();
            output.push_str(&format!("while ({}) {{\n", convert_pythonic_expr(cond)));
            let (b, n) = collect_and_parse_body(&lines[i + 1..], indent)?;
            output.push_str(&b);
            output.push_str("}\n");
            i += 1 + n;
            continue;
        }

        if trimmed == "try:" || trimmed.starts_with("try ") {
            output.push_str("try {\n");
            let (b, n) = collect_and_parse_body(&lines[i + 1..], indent)?;
            output.push_str(&b);
            i += 1 + n;

            let mut has_except = false;
            while i < total {
                let pl = lines[i];
                let pt = pl.trim();
                let pi = count_indent(pl);
                if pi == indent {
                    if pt == "except:" || pt.starts_with("except ") {
                        let exc_name = if pt == "except:" {
                            "_e".to_string()
                        } else {
                            let r = pt.strip_prefix("except ").unwrap().trim_end_matches(':');
                            if let Some(as_pos) = r.find(" as ") {
                                r[as_pos + 4..].trim().to_string()
                            } else {
                                r.to_string()
                            }
                        };
                        output.push_str(&format!("}} catch({}) {{\n", exc_name));
                        has_except = true;
                        let (eb, en) = collect_and_parse_body(&lines[i + 1..], indent)?;
                        output.push_str(&eb);
                        i += 1 + en;
                    } else if pt.starts_with("finally:") || pt == "finally:" {
                        if !has_except {
                            output.push_str("} catch(_e) {\n throw _e;\n");
                        }
                        output.push_str("} finally {\n");
                        let (fb, fn_) = collect_and_parse_body(&lines[i + 1..], indent)?;
                        output.push_str(&fb);
                        i += 1 + fn_;
                        break;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            if !has_except {
                output.push_str("} catch(_e) {\n throw _e;\n");
            }
            output.push_str("}\n");
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("return") {
            let val = rest.trim();
            if val.is_empty() {
                output.push_str("return;\n");
            } else {
                output.push_str(&format!("return {};\n", convert_pythonic_expr(val)));
            }
            i += 1;
            continue;
        }

        if trimmed == "pass" {
            i += 1;
            continue;
        }
        if trimmed == "break" {
            output.push_str("break;\n");
            i += 1;
            continue;
        }
        if trimmed == "continue" {
            output.push_str("continue;\n");
            i += 1;
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("raise ") {
            output.push_str(&format!("throw {};\n", convert_pythonic_expr(rest.trim())));
            i += 1;
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("del ") {
            output.push_str(&format!("delete {};\n", convert_pythonic_expr(rest.trim())));
            i += 1;
            continue;
        }

        let mut stmt = trimmed.to_string();
        let extra_lines = merge_continuation(&lines, i, &mut stmt);
        output.push_str(&maybe_add_var(&stmt));
        output.push_str(";\n");
        i += 1 + extra_lines;
    }

    Ok(output)
}

fn maybe_add_var(stmt: &str) -> String {
    let s = stmt.trim();
    if let Some(eq_pos) = s.find('=') {
        if eq_pos > 0 {
            let prev_char = s.chars().nth(eq_pos - 1).unwrap_or(' ');
            let is_compound = "+-*/&|^!<>".contains(prev_char);
            if !is_compound {
                let lhs = s[..eq_pos].trim();
                if !lhs.is_empty()
                    && !lhs.contains('.')
                    && !lhs.contains('[')
                    && !lhs.contains('(')
                    && lhs.chars().all(|c| c.is_alphanumeric() || c == '_')
                    && lhs.chars().next().map_or(false, |c| !c.is_ascii_digit())
                {
                    let rhs = s[eq_pos + 1..].trim();
                    if !contains_standalone_ident(rhs, lhs) {
                        return format!("var {} = {}", lhs, convert_pythonic_expr(rhs));
                    }
                }
            }
        }
    }
    convert_pythonic_expr(s)
}

fn contains_standalone_ident(haystack: &str, needle: &str) -> bool {
    let chars: Vec<char> = haystack.chars().collect();
    let needle_chars: Vec<char> = needle.chars().collect();
    if needle_chars.is_empty() || chars.is_empty() {
        return false;
    }
    let mut pos = 0;
    while pos + needle_chars.len() <= chars.len() {
        let mut matches = true;
        for (j, &nc) in needle_chars.iter().enumerate() {
            if chars[pos + j] != nc {
                matches = false;
                break;
            }
        }
        if matches {
            let before = pos == 0 || !chars[pos - 1].is_alphanumeric() && chars[pos - 1] != '_';
            let after = pos + needle_chars.len() >= chars.len()
                || !chars[pos + needle_chars.len()].is_alphanumeric()
                    && chars[pos + needle_chars.len()] != '_';
            if before && after {
                return true;
            }
        }
        pos += 1;
    }
    false
}

fn collect_and_parse_body(lines: &[&str], parent_indent: usize) -> Result<(String, usize), String> {
    let mut body_lines: Vec<&str> = Vec::new();
    let mut consumed = 0;
    for line in lines {
        if line.trim().is_empty() {
            body_lines.push(*line);
            consumed += 1;
            continue;
        }
        let indent = count_indent(line);
        if indent <= parent_indent {
            break;
        }
        body_lines.push(*line);
        consumed += 1;
    }
    let body_base = body_lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| count_indent(l))
        .min()
        .unwrap_or(parent_indent + 4);
    let body_js = parse_pythonic_block(&body_lines, body_base)?;
    Ok((body_js, consumed))
}

fn count_indent(line: &str) -> usize {
    line.chars().take_while(|c| c.is_whitespace()).count()
}

fn bracket_depth(s: &str) -> i32 {
    let mut d = 0i32;
    let mut in_str = false;
    for ch in s.chars() {
        if ch == '"' {
            in_str = !in_str;
            continue;
        }
        if in_str {
            continue;
        }
        match ch {
            '[' | '{' | '(' => d += 1,
            ']' | '}' | ')' => d -= 1,
            _ => {}
        }
    }
    d
}

fn merge_continuation(lines: &[&str], start: usize, stmt: &mut String) -> usize {
    let depth = bracket_depth(stmt);
    if depth <= 0 {
        return 0;
    }
    let mut extra = 0usize;
    let mut rem = depth;
    for &l in lines.iter().skip(start + 1) {
        let nt = l.trim();
        if nt.is_empty() {
            extra += 1;
            continue;
        }
        if nt.starts_with("def ")
            || nt.starts_with("if ")
            || nt.starts_with("for ")
            || nt.starts_with("while ")
            || nt.starts_with("try")
            || nt == "pass"
            || nt.starts_with("return")
            || nt.starts_with("raise")
            || nt.starts_with("del ")
            || nt.starts_with("break")
            || nt.starts_with("continue")
            || nt == "else:"
            || nt.starts_with("elif ")
            || nt.starts_with("except")
            || nt.starts_with("finally")
        {
            break;
        }
        stmt.push(' ');
        stmt.push_str(nt);
        rem += bracket_depth(nt);
        extra += 1;
        if rem <= 0 {
            break;
        }
    }
    extra
}

fn convert_component_blocks_inline(source: &str) -> Result<String, String> {
    let lines: Vec<&str> = source.lines().collect();
    let mut output = String::new();
    let mut i = 0;
    let mut in_raw_qml = false;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        if !in_raw_qml && trimmed == "/*@qml" {
            in_raw_qml = true;
            output.push_str(line);
            output.push('\n');
            i += 1;
            continue;
        }

        if in_raw_qml {
            output.push_str(line);
            output.push('\n');
            i += 1;
            if trimmed == "@qml*/" {
                in_raw_qml = false;
            }
            continue;
        }

        if trimmed.starts_with("component ") {
            let (qml, consumed) = transpile_component_inline(&lines[i..])?;
            output.push_str(&qml);
            if !qml.ends_with('\n') {
                output.push('\n');
            }
            i += consumed;
        } else {
            output.push_str(line);
            output.push('\n');
            i += 1;
        }
    }

    Ok(output)
}

fn transpile_component_inline(lines: &[&str]) -> Result<(String, usize), String> {
    let first = lines[0].trim();
    let after = first.strip_prefix("component ").unwrap().trim();

    let consumed: usize;
    let (name, args) = if let Some(paren) = after.find('(') {
        let n = after[..paren].trim();
        let close_paren = after[paren..].find(')').unwrap_or(after.len() - paren);
        let args_str = &after[paren + 1..paren + close_paren];
        let a: Vec<&str> = args_str
            .split(',')
            .map(|a| a.trim())
            .filter(|a| !a.is_empty())
            .collect();
        consumed = find_component_body_end(lines);
        (n.to_string(), a.iter().map(|s| s.to_string()).collect())
    } else {
        let n = after.trim_end_matches(':').trim().to_string();
        consumed = find_component_body_end(lines);
        (n, Vec::new())
    };

    let safe_id = to_lowercase_first(&name);
    COMPONENT_MAP.with(|cm| {
        cm.borrow_mut()
            .insert(name.clone(), (safe_id.clone(), args.clone()));
    });

    let mut jsx_lines = String::new();
    for line in lines.iter().take(consumed).skip(1) {
        jsx_lines.push_str(line);
        jsx_lines.push('\n');
    }

    let (jsx_qml, _) = jsx_to_qml_inner(&jsx_lines, 0)?;

    let body = if args.is_empty() {
        jsx_qml
    } else {
        inject_component_properties(&jsx_qml, &args.iter().map(|s| s.as_str()).collect::<Vec<&str>>())
    };

    let qml = format!("Component {{ id: {}\n    {}\n}}", safe_id, body);
    Ok((qml, consumed))
}

fn find_component_body_end(lines: &[&str]) -> usize {
    for j in 1..lines.len() {
        if lines[j].trim().starts_with('<') {
            let mut depth = count_jsx_depth(lines[j], 0);
            for k in (j + 1)..lines.len() {
                depth = count_jsx_depth(lines[k], depth);
                if depth <= 0 && lines[k].contains('>') {
                    return k + 1;
                }
            }
            return lines.len();
        }
    }
    lines.len()
}

fn inject_component_properties(jsx_qml: &str, args: &[&str]) -> String {
    if let Some(brace) = jsx_qml.find('{') {
        let before = &jsx_qml[..=brace];
        let after = &jsx_qml[brace + 1..];
        let mut result = before.to_string();
        for a in args {
            result.push_str(&format!("\n        property var {}", a));
        }
        result.push_str(after);
        result
    } else {
        let mut s = jsx_qml.to_string();
        for a in args {
            s.push_str(&format!("\n    property var {}", a));
        }
        s
    }
}

fn to_lowercase_first(s: &str) -> String {
    let mut chars: Vec<char> = s.chars().collect();
    if let Some(first) = chars.first() {
        if first.is_uppercase() {
            chars[0] = first.to_lowercase().next().unwrap_or(*first);
        }
    }
    chars.into_iter().collect()
}

fn convert_all_jsx_to_qml(source: &str) -> Result<String, String> {
    let mut output = String::new();
    let mut in_string = false;
    let mut in_raw_qml = false;
    let chars: Vec<char> = source.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];

        if !in_string
            && !in_raw_qml
            && ch == '/'
            && i + 7 < chars.len()
            && chars[i + 1] == '*'
            && chars[i + 2] == '@'
            && chars[i + 3] == 'q'
            && chars[i + 4] == 'm'
            && chars[i + 5] == 'l'
            && chars[i + 6] == '\n'
        {
            in_raw_qml = true;
            i += 7;
            continue;
        }
        if in_raw_qml {
            if ch == '@'
                && i + 5 < chars.len()
                && chars[i + 1] == 'q'
                && chars[i + 2] == 'm'
                && chars[i + 3] == 'l'
                && chars[i + 4] == '*'
                && chars[i + 5] == '/'
            {
                in_raw_qml = false;
                i += 6;
                continue;
            }
            output.push(ch);
            i += 1;
            continue;
        }

        if ch == '"' {
            in_string = !in_string;
            output.push(ch);
            i += 1;
            continue;
        }

        if in_string {
            output.push(ch);
            i += 1;
            continue;
        }

        if ch == '<' && i + 1 < chars.len() {
            let next = chars[i + 1];
            let looks_like_jsx = next.is_ascii_alphanumeric() || next == '.' || next == '_';

            if looks_like_jsx {
                let byte_offset = char_pos_to_byte(&chars, i);
                let remaining = &source[byte_offset..];
                match jsx_to_qml_inner(remaining, 0) {
                    Ok((qml, consumed)) => {
                        output.push_str(&qml);
                        i = byte_pos_to_char_index(&chars, byte_offset + consumed);
                        continue;
                    }
                    Err(_) => {
                        output.push(ch);
                        i += 1;
                    }
                }
            } else {
                output.push(ch);
                i += 1;
            }
        } else {
            output.push(ch);
            i += 1;
        }
    }

    Ok(output)
}

fn process_text_buf(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let mut result = String::new();
    for line in trimmed.lines() {
        if !result.is_empty() {
            result.push('\n');
        }
        let lt = line.trim();
        if lt.starts_with('{') && lt.ends_with('}') && lt.len() > 2 {
            let trimmed_no_ws: String = lt.chars().filter(|c| !c.is_whitespace()).collect();
            if trimmed_no_ws.starts_with('{') && trimmed_no_ws.ends_with('}') && trimmed_no_ws.len() > 2 {
                let inner = lt[1..lt.len() - 1].trim();
                result.push_str(&format!("data: {}", inner));
            } else {
                result.push_str(line);
            }
        } else {
            result.push_str(line);
        }
    }
    result
}

fn jsx_to_qml_inner(s: &str, depth: i32) -> Result<(String, usize), String> {
    if depth > 1000 {
        return Err("Max recursion depth exceeded in JSX parsing (possible circular nesting)".to_string());
    }
    let trimmed = s.trim_start();
    let leading_whitespace = s.len() - trimmed.len();
    if !trimmed.starts_with('<') {
        return Err("Expected JSX element".to_string());
    }

    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() < 2 {
         return Err("Tag structure is empty".to_string());
    }
    let mut i = 1usize;

    let tag_start = i;
    while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '.' || chars[i] == '_') {
        i += 1;
    }
    if i == tag_start {
        return Err("Empty tag name".to_string());
    }
    let tag_name: String = chars[tag_start..i].iter().collect();
    let mut attrs: Vec<(String, String, bool, bool)> = Vec::new();

    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    if i < chars.len() && chars[i] == '(' {
        i += 1;
        let key_start = i;
        let mut paren_depth = 1;
        while i < chars.len() && paren_depth > 0 {
            if chars[i] == '(' {
                paren_depth += 1
            } else if chars[i] == ')' {
                paren_depth -= 1;
                if paren_depth == 0 {
                    break;
                }
            }
            i += 1;
        }
        let key_value: String = chars[key_start..i].iter().collect();
        let key_trimmed = key_value.trim();
        if key_trimmed.starts_with('{') && key_trimmed.ends_with('}') {
            let inner = key_trimmed[1..key_trimmed.len() - 1].trim();
            attrs.push(("key".to_string(), inner.to_string(), true, false));
        } else {
            attrs.push(("key".to_string(), key_trimmed.to_string(), false, false));
        }
        i += 1;
    }

    let mut in_attr_value = false;
    let mut attr_name = String::new();
    let mut attr_value = String::new();
    let mut is_expr = false;

    while i < chars.len() {
        if in_attr_value {
            if is_expr {
                let mut brace_depth = 1;
                i += 1;
                while i < chars.len() && brace_depth > 0 {
                    match chars[i] {
                        '"' => {
                            attr_value.push('"');
                            i += 1;
                            while i < chars.len() && chars[i] != '"' {
                                attr_value.push(chars[i]);
                                i += 1;
                            }
                            if i < chars.len() {
                                attr_value.push('"');
                                i += 1;
                            }
                            continue;
                        }
                        '{' => {
                            brace_depth += 1;
                            attr_value.push('{');
                            i += 1;
                        }
                        '}' => {
                            brace_depth -= 1;
                            if brace_depth == 0 {
                                break;
                            }
                            attr_value.push('}');
                            i += 1;
                        }
                        _ => {
                            attr_value.push(chars[i]);
                            i += 1;
                        }
                    }
                }
                if !attr_name.is_empty() {
                    let is_custom = attr_name.starts_with('@');
                    if is_custom {
                        attr_name = attr_name[1..].to_string();
                    }
                    if is_jsx_start(&attr_value) {
                        let id = inline_jsx_to_component(&attr_value)?;
                        attrs.push((attr_name.clone(), id, false, is_custom));
                    } else {
                        attrs.push((attr_name.clone(), convert_inline_expr(&attr_value), true, is_custom));
                    }
                }
                attr_name.clear();
                attr_value.clear();
                is_expr = false;
                in_attr_value = false;
                i += 1;
            } else {
                if chars[i] == '"' {
                    if !attr_name.is_empty() {
                        let is_custom = attr_name.starts_with('@');
                        if is_custom {
                            attr_name = attr_name[1..].to_string();
                        }
                        attrs.push((attr_name.clone(), attr_value.clone(), false, is_custom));
                    }
                    attr_name.clear();
                    attr_value.clear();
                    in_attr_value = false;
                } else {
                    attr_value.push(chars[i]);
                }
                i += 1;
            }
            continue;
        }

        if chars[i] == '/' && i + 1 < chars.len() && chars[i + 1] == '>' {
            if let Some((cid, _)) = is_registered_component(&tag_name) {
                let qml = emit_component_loader(&cid, &attrs);
                let consumed = leading_whitespace + char_pos_to_byte(&chars, i + 2);
                return Ok((qml, consumed));
            }
            if tag_name.contains('.') {
                let consumed = leading_whitespace + char_pos_to_byte(&chars, i + 2);
                return Ok((String::new(), consumed));
            }
            let (tag_suffix, skip_on) = behavior_tag_suffix(&tag_name, &attrs);
            let mut qml = format!("{}{} {{\n", tag_name, tag_suffix);
            for (n, v, expr, is_custom) in &attrs {
                if skip_on && is_behavior_on_attr(n) {
                    continue;
                }
                let attr = format_attr(&tag_name, n, v, *expr, *is_custom);
                qml.push_str(&format!("    {}\n", attr));
            }
            qml.push('}');
            let consumed = leading_whitespace + char_pos_to_byte(&chars, i + 2);
            return Ok((qml, consumed));
        }

        if chars[i] == '>' {
            i += 1;
            break;
        }
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }

        let an_start = i;
        let is_at_prefix = i < chars.len() && chars[i] == '@';
        if is_at_prefix {
            i += 1;
        }
        while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '-') {
            i += 1;
        }
        attr_name = chars[an_start..i].iter().collect();

        if attr_name.is_empty() || attr_name == "@" {
            i += 1;
            continue;
        }

        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i < chars.len() && chars[i] == '=' {
            i += 1;
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            if i < chars.len() && chars[i] == '{' {
                is_expr = true;
                in_attr_value = true;
            } else if i < chars.len() && chars[i] == '"' {
                is_expr = false;
                i += 1;
                in_attr_value = true;
            }
        } else {
            let is_custom = attr_name.starts_with('@');
            if is_custom {
                attr_name = attr_name[1..].to_string();
            }
            attrs.push((attr_name.clone(), "true".to_string(), false, is_custom));
            attr_name.clear();
        }
    }

    let mut children_qml = String::new();
    let mut text_buf = String::new();

    while i < chars.len() {
        if chars[i] != '<' {
            text_buf.push(chars[i]);
            i += 1;
            continue;
        }

        let is_closing = i + 1 < chars.len() && chars[i + 1] == '/';
        let is_opening = i + 1 < chars.len()
            && chars[i + 1] != '/'
            && chars[i + 1] != ' '
            && chars[i + 1] != '\t'
            && (chars[i + 1].is_ascii_alphanumeric() || chars[i + 1] == '.' || chars[i + 1] == '_');

        if !is_closing && !is_opening {
            text_buf.push('<');
            i += 1;
            continue;
        }

        if !text_buf.trim().is_empty() {
            children_qml.push_str(&process_text_buf(&text_buf));
            children_qml.push('\n');
            text_buf.clear();
        }

        if is_closing {
            let close_start = i;
            let remaining = &trimmed[char_pos_to_byte(&chars, i)..];
            let close_end = remaining.find('>').ok_or("Unclosed tag")?;
            let close_tag = remaining[2..close_end].trim();
            if close_tag == tag_name.as_str() {
                if !text_buf.trim().is_empty() {
                    children_qml.push_str(&process_text_buf(&text_buf));
                    children_qml.push('\n');
                }
                if let Some((cid, _)) = is_registered_component(&tag_name) {
                    let qml = emit_component_loader_with_children(&cid, &attrs, &children_qml);
                    let consumed = leading_whitespace + char_pos_to_byte(&chars, close_start) + close_end + 1;
                    return Ok((qml, consumed));
                }
                if tag_name.contains('.') {
                    let parts: Vec<&str> = tag_name.splitn(2, '.').collect();
                    let prop_name = parts[1];
                    let mut wrap_type = "Item".to_string();
                    for (n, v, _, _) in &attrs {
                        if n == "wrap" {
                            wrap_type = v.clone();
                            break;
                        }
                    }
                    if wrap_type == "none" {
                        let consumed = leading_whitespace + char_pos_to_byte(&chars, close_start) + close_end + 1;
                        return Ok((children_qml.trim().to_string(), consumed));
                    }
                    let mut wrapped = format!("{}: {} {{\n", prop_name, wrap_type);
                    for line in children_qml.lines() {
                        let trimmed_line = line.trim();
                        if !trimmed_line.is_empty() {
                            wrapped.push_str(&format!("    {}\n", trimmed_line));
                        }
                    }
                    wrapped.push('}');
                    let consumed = leading_whitespace + char_pos_to_byte(&chars, close_start) + close_end + 1;
                    return Ok((wrapped, consumed));
                }
                let is_property = tag_name.chars().next().map_or(false, |c| c.is_lowercase())
                    && attrs.iter().all(|(n, _, _, _)| n == "id");
                if is_property {
                    let mut qml = format!("{}:", tag_name);
                    if !children_qml.is_empty() {
                        qml.push_str(" Component {\n");
                        for child_line in children_qml.trim().lines() {
                            let ct = child_line.trim();
                            if !ct.is_empty() {
                                qml.push_str(&format!("    {}\n", ct));
                            }
                        }
                        qml.push('}');
                    }
                    let consumed = leading_whitespace + char_pos_to_byte(&chars, close_start) + close_end + 1;
                    return Ok((qml, consumed));
                }
                let (tag_suffix, skip_on) = behavior_tag_suffix(&tag_name, &attrs);
                let mut qml = format!("{}{} {{\n", tag_name, tag_suffix);
                for (n, v, expr, is_custom) in &attrs {
                    if skip_on && is_behavior_on_attr(n) {
                        continue;
                    }
                    let attr = format_attr(&tag_name, n, v, *expr, *is_custom);
                    qml.push_str(&format!("    {}\n", attr));
                }
                if !children_qml.is_empty() {
                    for child_line in children_qml.lines() {
                        let ct = child_line.trim();
                        if !ct.is_empty() {
                            qml.push_str(&format!("    {}\n", ct));
                        }
                    }
                }
                qml.push('}');
                let consumed = leading_whitespace + char_pos_to_byte(&chars, close_start) + close_end + 1;
                return Ok((qml, consumed));
            }
            text_buf.push('<');
            text_buf.push('/');
            i += 1;
            while i < chars.len() && chars[i] != '>' {
                text_buf.push(chars[i]);
                i += 1;
            }
            if i < chars.len() {
                text_buf.push('>');
                i += 1;
            }
            continue;
        }

        if i + 1 < chars.len() && chars[i + 1] != '/' && chars[i + 1] != ' ' && chars[i + 1] != '\t' {
            let byte_offset = char_pos_to_byte(&chars, i);
            let child_sub = &trimmed[byte_offset..];
            match jsx_to_qml_inner(child_sub, depth + 1) {
                Ok((child_qml, child_consumed)) => {
                    if child_consumed == 0 {
                        text_buf.push('<');
                        i += 1;
                    } else {
                        children_qml.push_str(&child_qml);
                        children_qml.push('\n');
                        let bytes_skipped = byte_offset + child_consumed;
                        let next_i = byte_pos_to_char_index(&chars, bytes_skipped);
                        if next_i <= i {
                            i += 1;
                        } else {
                            i = next_i;
                        }
                    }
                    continue;
                }
                Err(_) => {
                    text_buf.push('<');
                    i += 1;
                }
            }
        } else {
            text_buf.push('<');
            i += 1;
        }
    }

    if !text_buf.trim().is_empty() {
        children_qml.push_str(&process_text_buf(&text_buf));
    }

    let (tag_suffix, skip_on) = behavior_tag_suffix(&tag_name, &attrs);
    let mut qml = format!("{}{} {{\n", tag_name, tag_suffix);
    for (n, v, expr, is_custom) in &attrs {
        if skip_on && is_behavior_on_attr(n) {
            continue;
        }
        let attr = format_attr(&tag_name, n, v, *expr, *is_custom);
        qml.push_str(&format!("    {}\n", attr));
    }
    qml.push('}');
    Ok((qml, leading_whitespace + char_pos_to_byte(&chars, i)))
}

fn format_attr(tag_name: &str, name: &str, value: &str, is_expr: bool, is_custom: bool) -> String {
    if tag_name == "Behavior"
        && name.len() > 2
        && name.starts_with("on")
        && name.chars().nth(2).unwrap_or(' ').is_uppercase()
    {
        let prop = &name[2..];
        let first = prop.chars().next().unwrap().to_lowercase().to_string();
        let rest: String = prop.chars().skip(first.len()).collect();
        return format!("on {}", first + &rest);
    }

    if tag_name == "Connections"
        && name.len() > 2
        && name.starts_with("on")
        && name.chars().nth(2).unwrap_or(' ').is_uppercase()
    {
        let trimmed = value.trim();
        if let Some(paren) = trimmed.find('(') {
            let args_and_body = &trimmed[paren..];
            return format!("function {}{}", name, args_and_body);
        }
    }

    if tag_name == "State.Context" && name == "as" {
        if is_expr {
            return format!("key: {}", convert_pythonic_expr(value));
        }
        return format!("key: \"{}\"", value);
    }
    if name == "id" {
        return format!("id: {}", value);
    }

    let val = if is_expr {
        let t = value.trim();
        if t == "{}" {
            "({})".to_string()
        } else if is_qml_object_literal(t) {
            value.to_string()
        } else if t.starts_with('{') {
            format!("({})", value)
        } else {
            convert_pythonic_expr(value)
        }
    } else {
        let qml_name = camel_to_dot(name);
        if value == "true" || value == "false" {
            if qml_name.starts_with("anchors.") {
                if value == "true" {
                    let prop = &qml_name["anchors.".len()..];
                    match prop {
                        "fill" | "centerIn" => "parent".to_string(),
                        _ => format!("parent.{}", prop),
                    }
                } else {
                    value.to_string()
                }
            } else {
                value.to_string()
            }
        } else if value == "parent" && qml_name.starts_with("anchors.") {
            let prop = &qml_name["anchors.".len()..];
            match prop {
                "fill" | "centerIn" => "parent".to_string(),
                _ => format!("parent.{}", prop),
            }
        } else {
            format!("\"{}\"", value)
        }
    };

    let qml_name = camel_to_dot(name);
    if is_custom {
        return format!("property var {}: {}", qml_name, val);
    }

    format!("{}: {}", qml_name, val)
}

fn is_qml_object_literal(expr: &str) -> bool {
    let t = expr.trim();
    let mut chars = t.chars().peekable();
    match chars.next() {
        Some(c) if c.is_ascii_uppercase() => {}
        _ => return false,
    }
    while let Some(&c) = chars.peek() {
        if c.is_ascii_alphanumeric() || c == '_' {
            chars.next();
        } else {
            break;
        }
    }
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else {
            break;
        }
    }
    chars.peek() == Some(&'{')
}

fn is_custom_property(_tag_name: &str, prop_name: &str) -> bool {
    let standard_props = [
        "id", "objectName", "parent", "children", "data", "resources",
        "state", "states", "transitions", "width", "height", "x", "y", "z",
        "visible", "opacity", "enabled", "focus", "activeFocus", "activeFocusOnTab",
        "clip", "smooth", "antialiasing", "layer", "transform", "rotation", "scale",
        "transformOrigin", "implicitWidth", "implicitHeight", "baselineOffset",
        "flags", "color", "title", "modality", "visibility", "screen",
        "transientParent", "contentOrientation", "active", "activeFocusItem",
        "minimumWidth", "minimumHeight", "maximumWidth", "maximumHeight",
        "radius", "border", "gradient",
        "text", "font", "horizontalAlignment", "verticalAlignment", "wrapMode",
        "elide", "style", "styleColor", "lineHeight", "lineHeightMode",
        "minimumPixelSize", "minimumPointSize", "renderType", "textFormat",
        "spacing", "layoutDirection",
        "hoverEnabled", "pressed", "containsMouse", "cursorShape",
        "running", "loops", "loopCount", "alwaysRunToEnd", "paused",
        "onClicked", "onPressed", "onReleased", "onDoubleClicked", "onPositionChanged",
        "onEntered", "onExited", "onCanceled", "onCompleted", "onDestruction",
        "onWidthChanged", "onHeightChanged", "onXChanged", "onYChanged",
        "onVisibleChanged", "onOpacityChanged", "onEnabledChanged",
    ];

    if prop_name.starts_with('_') {
        return true;
    }
    if standard_props.contains(&prop_name) {
        return false;
    }

    let grouped_prefixes = ["font", "border", "anchors", "Layout", "layout"];
    for prefix in &grouped_prefixes {
        let prefix_lower = prefix.to_lowercase();
        let prop_lower = prop_name.to_lowercase();
        if prop_lower.starts_with(&prefix_lower) && prop_name.len() > prefix.len() {
            let next_char = prop_name.chars().nth(prefix.len()).unwrap_or(' ');
            if next_char.is_uppercase() || prop_name.starts_with(&format!("{}.", prefix)) {
                return false;
            }
        }
    }

    true
}

fn is_registered_component(name: &str) -> Option<(String, Vec<String>)> {
    COMPONENT_MAP.with(|cm| cm.borrow().get(name).cloned())
}

fn emit_component_loader(component_id: &str, attrs: &[(String, String, bool, bool)]) -> String {
    if attrs.is_empty() {
        return format!("Loader Tint {{\n    sourceComponent: {}\n}}", component_id);
    }
    let mut qml = format!("Loader {{\n    sourceComponent: {}\n    onLoaded: {{\n", component_id);
    for (n, v, expr, _) in attrs {
        let val = if *expr {
            let trimmed = v.trim();
            if trimmed.starts_with("function") || trimmed.starts_with("function(") {
                v.clone()
            } else {
                convert_pythonic_expr(v)
            }
        } else {
            format!("\"{}\"", v)
        };
        qml.push_str(&format!("        item.{} = {}\n", camel_to_dot(n), val));
    }
    qml.push_str("    }\n}");
    qml
}

fn emit_component_loader_with_children(component_id: &str, attrs: &[(String, String, bool, bool)], children: &str) -> String {
    let mut base = emit_component_loader(component_id, attrs);
    if !children.trim().is_empty() {
        if let Some(brace) = base.rfind('}') {
            let mut modified = base[..brace].to_string();
            modified.push_str("    ");
            modified.push_str(children.trim());
            modified.push('\n');
            modified.push('}');
            base = modified;
        }
    }
    base
}

fn count_jsx_depth(line: &str, start_depth: i32) -> i32 {
    let trimmed = line.trim();
    let mut d = 0i32;
    let mut in_string = false;
    let chars: Vec<char> = trimmed.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '"' {
            in_string = !in_string;
        }
        if !in_string {
            if chars[i] == '<' && i + 1 < chars.len() && chars[i + 1] != '/' && chars[i + 1] != ' ' && chars[i + 1] != '\t' {
                d += 1;
            } else if chars[i] == '/' && i + 1 < chars.len() && chars[i + 1] == '>' {
                d -= 1;
            } else if chars[i] == '<' && i + 1 < chars.len() && chars[i + 1] == '/' {
                d -= 1;
            }
        }
        i += 1;
    }
    start_depth + d
}

fn behavior_tag_suffix(tag_name: &str, attrs: &[(String, String, bool, bool)]) -> (String, bool) {
    if tag_name != "Behavior" {
        return (String::new(), false);
    }
    for (n, _, _, _) in attrs {
        if is_behavior_on_attr(n) {
            let prop = &n[2..];
            let first = prop.chars().next().unwrap().to_lowercase().to_string();
            let rest: String = prop.chars().skip(first.len()).collect();
            return (format!(" on {}{}", first, rest), true);
        }
    }
    (String::new(), false)
}

fn is_behavior_on_attr(name: &str) -> bool {
    name.len() > 2 && name.starts_with("on") && name.chars().nth(2).unwrap_or(' ').is_uppercase()
}

fn camel_to_dot(name: &str) -> String {
    if name == "id" || name == "name" || name == "objectName" || name == "width" || name == "height" {
        return name.to_string();
    }
    if name.starts_with("on") && name.len() > 2 && name.chars().nth(2).unwrap_or(' ').is_uppercase() {
        return name.to_string();
    }

    let known_groups: &[(&str, &[&str])] = &[
        ("anchors", &["fill", "centerIn", "top", "bottom", "left", "right",
                       "horizontalCenter", "verticalCenter", "baseline",
                       "topMargin", "bottomMargin", "leftMargin", "rightMargin",
                       "horizontalCenterOffset", "verticalCenterOffset",
                       "baselineOffset", "margins"]),
        ("font", &["family", "pixelSize", "weight", "bold", "italic", "underline",
                    "strikeout", "pointSize", "letterSpacing", "wordSpacing",
                    "capitalization", "hintingPreference", "kerning", "preferShaping"]),
        ("border", &["color", "width"]),
        ("Layout", &["fillWidth", "fillHeight", "preferredWidth", "preferredHeight",
                      "minimumWidth", "minimumHeight", "maximumWidth", "maximumHeight",
                      "alignment", "column", "row", "columnSpan", "rowSpan",
                      "margins", "topMargin", "bottomMargin", "leftMargin", "rightMargin"]),
        ("Font", &["Bold", "Normal", "Medium", "Light", "DemiBold", "Black", "Thin",
                    "ExtraLight", "ExtraBold"]),
        ("Anchors", &["fill", "centerIn"]),
        ("Component", &[]),
    ];

    let lower_full = name.to_lowercase();
    for (grp, subs) in known_groups {
        let grp_lower = grp.to_lowercase();
        if name.starts_with(grp) || lower_full.starts_with(&grp_lower) {
            let rest = &name[grp.len()..];
            if rest.is_empty() {
                return grp_lower;
            }
            for sub in *subs {
                let sub_lower = sub.to_lowercase();
                if rest == *sub || rest.to_lowercase() == sub_lower {
                    let first_char = rest.chars().next().unwrap();
                    let offset = first_char.len_utf8();
                    let rest_lower = first_char.to_lowercase().to_string() + &rest[offset..];
                    return format!("{}.{}", grp, rest_lower);
                }
            }
            return name.to_string();
        }
    }
    name.to_string()
}

fn char_pos_to_byte(chars: &[char], char_pos: usize) -> usize {
    chars.iter().take(char_pos).map(|c| c.len_utf8()).sum()
}

fn byte_pos_to_char_index(chars: &[char], byte_pos: usize) -> usize {
    let mut bytes = 0;
    for (i, c) in chars.iter().enumerate() {
        if bytes >= byte_pos {
            return i;
        }
        bytes += c.len_utf8();
    }
    chars.len()
}

fn fix_root_on_completed(output: &str) -> String {
    let lines: Vec<&str> = output.lines().collect();
    let mut result = String::new();
    let mut in_root = false;
    let mut root_depth = 0i32;
    let mut fixed_completed = false;
    let mut fixed_destruction = false;

    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();

        if !in_root && !trimmed.starts_with("import ") && !trimmed.is_empty() {
            if trimmed.contains('{') {
                in_root = true;
                root_depth = count_braces_on_line(line);
            }
        } else if in_root {
            root_depth += count_braces_on_line(line);
            if root_depth <= 0 {
                in_root = false;
            }

            let stripped = line.trim_start();
            if stripped.starts_with("onCompleted:") && !fixed_completed {
                let indent = &line[..line.len() - stripped.len()];
                let after = stripped.strip_prefix("onCompleted:").unwrap().trim();
                result.push_str(&format!("{}Component.onCompleted: {}\n", indent, after));
                fixed_completed = true;
                continue;
            }
            if stripped.starts_with("onDestruction:") && !fixed_destruction {
                let indent = &line[..line.len() - stripped.len()];
                let after = stripped.strip_prefix("onDestruction:").unwrap().trim();
                result.push_str(&format!("{}Component.onDestruction: {}\n", indent, after));
                fixed_destruction = true;
                continue;
            }
        }

        result.push_str(line);
        if idx + 1 < lines.len() {
            result.push('\n');
        }
    }
    result
}

fn nest_components_inside_window(output: &str) -> String {
    let lines: Vec<&str> = output.lines().collect();
    let mut root_open = None;
    let mut root_close = None;
    let mut depth = 0i32;

    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let is_import = trimmed.starts_with("import ");
        let is_component = trimmed.starts_with("Component {");
        let is_blank = trimmed.is_empty();

        if !is_import && !is_component && !is_blank && root_open.is_none() {
            if trimmed.contains('{') {
                root_open = Some(idx);
                depth = count_braces_on_line(line);
            }
        } else if root_open.is_some() {
            depth += count_braces_on_line(line);
            if depth <= 0 {
                root_close = Some(idx);
                break;
            }
        }
    }

    let close_idx = match root_close {
        Some(i) => i,
        None => return output.to_string(),
    };

    let mut skip_ranges: Vec<(usize, usize)> = Vec::new();
    let mut after_components: Vec<String> = Vec::new();
    let mut i = close_idx + 1;

    while i < lines.len() {
        let trimmed = lines[i].trim();
        if trimmed.starts_with("Component {") {
            let start_idx = i;
            let mut block = String::new();
            let mut bdepth = count_braces_on_line(lines[i]);
            block.push_str(lines[i]);
            i += 1;
            while i < lines.len() && bdepth > 0 {
                block.push('\n');
                block.push_str(lines[i]);
                bdepth += count_braces_on_line(lines[i]);
                i += 1;
            }
            skip_ranges.push((start_idx, i - 1));
            after_components.push(block);
        } else if trimmed.is_empty() {
            i += 1;
        } else {
            break;
        }
    }

    if after_components.is_empty() {
        return output.to_string();
    }

    let mut result = String::new();
    let mut j = 0;
    while j < lines.len() {
        let mut skip_to = None;
        for &(s, e) in &skip_ranges {
            if j >= s && j <= e {
                skip_to = Some(e + 1);
                break;
            }
        }
        if let Some(next) = skip_to {
            j = next;
            continue;
        }

        if j == close_idx {
            for comp in &after_components {
                result.push_str(comp);
                result.push('\n');
            }
        }

        result.push_str(lines[j]);
        if j + 1 < lines.len() {
            result.push('\n');
        }
        j += 1;
    }
    result
}

fn count_braces_on_line(line: &str) -> i32 {
    let mut count: i32 = 0;
    let mut in_string = false;
    let mut in_comment_multi = false;
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if in_string {
            if ch == '"' {
                in_string = false;
            }
        } else if in_comment_multi {
            if ch == '*' && i + 1 < chars.len() && chars[i + 1] == '/' {
                in_comment_multi = false;
                i += 2;
                continue;
            }
        } else if ch == '"' {
            in_string = true;
        } else if ch == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            break;
        } else if ch == '/' && i + 1 < chars.len() && chars[i + 1] == '*' {
            in_comment_multi = true;
            i += 2;
            continue;
        } else if ch == '{' {
            count += 1;
        } else if ch == '}' {
            count -= 1;
        }
        i += 1;
    }
    count
}

fn promote_imports(output: &str) -> String {
    let mut imports = Vec::new();
    let mut body_lines = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("import ") {
            imports.push(trimmed.to_string());
        } else {
            body_lines.push(line.to_string());
        }
    }
    if imports.is_empty() {
        return output.to_string();
    }
    let mut result = imports.join("\n");
    let first_body = body_lines.iter().position(|l| !l.trim().is_empty()).unwrap_or(0);
    if first_body < body_lines.len() {
        result.push('\n');
        for i in first_body..body_lines.len() {
            result.push_str(&body_lines[i]);
            if i + 1 < body_lines.len() {
                result.push('\n');
            }
        }
    }
    result
}

fn clean_output(output: &str) -> String {
    let mut result = String::new();
    let mut blank_count = 0u32;
    for line in output.lines() {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            blank_count += 1;
            if blank_count <= 2 {
                result.push('\n');
            }
        } else {
            blank_count = 0;
            let leading = line.len() - line.trim_start().len();
            let indent = " ".repeat(leading);
            result.push_str(&indent);
            result.push_str(trimmed.trim_start());
            result.push('\n');
        }
    }
    while result.ends_with('\n') {
        result.pop();
    }
    if !result.is_empty() {
        result.push('\n');
    }
    result
}
