//! Two-pass, Merlin-inspired macro assembler for the Fanticon NMOS 6502.

use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub source: String,
    pub line: usize,
    pub column: usize,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssembledProgram {
    pub origin: u16,
    pub bytes: Vec<u8>,
    pub symbols: BTreeMap<String, u16>,
}

#[derive(Clone)]
struct SourceLine {
    source: String,
    line: usize,
    text: String,
}

#[derive(Clone)]
struct Statement {
    source: String,
    line: usize,
    label: Option<String>,
    operation: Option<String>,
    operand: String,
}

#[derive(Clone)]
struct MacroDefinition {
    body: Vec<SourceLine>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Implied,
    Accumulator,
    Immediate,
    ZeroPage,
    ZeroPageX,
    ZeroPageY,
    Absolute,
    AbsoluteX,
    AbsoluteY,
    Indirect,
    IndirectX,
    IndirectY,
    Relative,
}

#[derive(Clone)]
enum PlanKind {
    Empty,
    Instruction { mnemonic: String, mode: Mode, expression: Option<String> },
    Bytes(Vec<String>),
    Words(Vec<String>),
    Ascii(Vec<String>),
    Hex(Vec<u8>),
    Reserve(String),
    Origin,
}

#[derive(Clone)]
struct PlannedLine {
    statement: Statement,
    address: u16,
    size: usize,
    kind: PlanKind,
}

pub fn assemble(source: &str) -> Result<AssembledProgram, Vec<Diagnostic>> {
    assemble_with_loader("<memory>", source, |_| Err("include files are unavailable".to_owned()))
}

pub fn assemble_with_loader<F>(
    source_name: &str,
    source: &str,
    mut loader: F,
) -> Result<AssembledProgram, Vec<Diagnostic>>
where
    F: FnMut(&str) -> Result<String, String>,
{
    let mut diagnostics = Vec::new();
    let root = source_lines(source_name, source);
    let included = expand_includes(root, &mut loader, &mut diagnostics, 0);
    let expanded = expand_macros(included, &mut diagnostics);
    let statements = expanded.iter().map(parse_statement).collect::<Vec<_>>();
    let (plan, symbols) = plan_program(&statements, &mut diagnostics);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    emit_program(&plan, symbols, &mut diagnostics)
        .and_then(|program| if diagnostics.is_empty() { Ok(program) } else { Err(diagnostics) })
}

fn source_lines(name: &str, source: &str) -> Vec<SourceLine> {
    source
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .split('\n')
        .enumerate()
        .map(|(index, text)| SourceLine {
            source: name.to_owned(),
            line: index + 1,
            text: text.to_owned(),
        })
        .collect()
}

fn expand_includes<F>(
    lines: Vec<SourceLine>,
    loader: &mut F,
    diagnostics: &mut Vec<Diagnostic>,
    depth: usize,
) -> Vec<SourceLine>
where
    F: FnMut(&str) -> Result<String, String>,
{
    if depth >= 16 {
        if let Some(line) = lines.first() {
            error(diagnostics, line, "include nesting exceeds 16 levels");
        }
        return Vec::new();
    }
    let mut output = Vec::new();
    for line in lines {
        let statement = parse_statement(&line);
        let include = statement
            .operation
            .as_deref()
            .is_some_and(|operation| matches!(operation, "PUT" | "USE" | "INCLUDE"));
        if !include {
            output.push(line);
            continue;
        }
        let path = statement.operand.trim().trim_matches(['\'', '"']);
        if path.is_empty() {
            error(diagnostics, &line, "include path is required");
            continue;
        }
        match loader(path) {
            Ok(source) => {
                let nested = source_lines(path, &source);
                output.extend(expand_includes(nested, loader, diagnostics, depth + 1));
            }
            Err(message) => error(diagnostics, &line, format!("cannot include {path}: {message}")),
        }
    }
    output
}

fn expand_macros(lines: Vec<SourceLine>, diagnostics: &mut Vec<Diagnostic>) -> Vec<SourceLine> {
    let mut definitions = HashMap::<String, MacroDefinition>::new();
    let mut ordinary = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let statement = parse_statement(&lines[index]);
        if statement.operation.as_deref() != Some("MAC") {
            ordinary.push(lines[index].clone());
            index += 1;
            continue;
        }
        let Some(name) = statement.label else {
            error(diagnostics, &lines[index], "MAC requires a macro name in the label field");
            index += 1;
            continue;
        };
        let mut body = Vec::new();
        index += 1;
        while index < lines.len() {
            let end = parse_statement(&lines[index]);
            if matches!(end.operation.as_deref(), Some("EOM" | "<<<")) {
                break;
            }
            body.push(lines[index].clone());
            index += 1;
        }
        if index == lines.len() {
            error(diagnostics, body.last().unwrap_or(&lines[index - 1]), "macro is missing EOM");
        } else {
            index += 1;
        }
        if definitions.insert(name.clone(), MacroDefinition { body }).is_some() {
            error(diagnostics, &lines[index.saturating_sub(1)], format!("duplicate macro {name}"));
        }
    }

    let mut output = Vec::new();
    for line in ordinary {
        expand_macro_line(&line, &definitions, &mut output, diagnostics, 0);
    }
    output
}

fn expand_macro_line(
    line: &SourceLine,
    definitions: &HashMap<String, MacroDefinition>,
    output: &mut Vec<SourceLine>,
    diagnostics: &mut Vec<Diagnostic>,
    depth: usize,
) {
    if depth >= 32 {
        error(diagnostics, line, "macro expansion exceeds 32 levels");
        return;
    }
    let statement = parse_statement(line);
    let (name, arguments) = match statement.operation.as_deref() {
        Some("PMC" | ">>>") => {
            let mut split = statement.operand.splitn(2, [',', ';', ' ']);
            (
                split.next().unwrap_or_default().to_ascii_uppercase(),
                split.next().unwrap_or_default(),
            )
        }
        Some(operation) if definitions.contains_key(operation) => {
            (operation.to_owned(), statement.operand.as_str())
        }
        _ => {
            output.push(line.clone());
            return;
        }
    };
    let Some(definition) = definitions.get(&name) else {
        error(diagnostics, line, format!("unknown macro {name}"));
        return;
    };
    let arguments = split_macro_arguments(arguments);
    for body_line in &definition.body {
        let mut text = body_line.text.clone();
        for parameter in (1..=8).rev() {
            let value = arguments.get(parameter - 1).map_or("", String::as_str);
            text = text.replace(&format!("]{parameter}"), value);
        }
        let expanded = SourceLine { source: line.source.clone(), line: line.line, text };
        expand_macro_line(&expanded, definitions, output, diagnostics, depth + 1);
    }
}

fn split_macro_arguments(arguments: &str) -> Vec<String> {
    let separator = if arguments.contains(';') { ';' } else { ',' };
    if arguments.trim().is_empty() {
        Vec::new()
    } else {
        arguments.split(separator).map(|argument| argument.trim().to_owned()).collect()
    }
}

fn parse_statement(line: &SourceLine) -> Statement {
    let macro_invocation = line
        .text
        .split_whitespace()
        .take(2)
        .any(|field| matches!(field.to_ascii_uppercase().as_str(), "PMC" | ">>>"));
    let code = if macro_invocation { line.text.as_str() } else { strip_comment(&line.text) };
    let trimmed = code.trim();
    if trimmed.is_empty() || trimmed.starts_with('*') {
        return Statement {
            source: line.source.clone(),
            line: line.line,
            label: None,
            operation: None,
            operand: String::new(),
        };
    }
    let leading = code.len() != code.trim_start().len();
    let fields = trimmed.split_whitespace().collect::<Vec<_>>();
    let first = fields[0].trim_end_matches(':').to_ascii_uppercase();
    let first_is_operation = is_known_operation(&first);
    let has_label = !leading && !first_is_operation;
    let (label, operation_index) = if has_label { (Some(first), 1) } else { (None, 0) };
    let operation = fields.get(operation_index).map(|field| field.to_ascii_uppercase());
    let operand = fields.get(operation_index + 1..).unwrap_or_default().join(" ");
    Statement { source: line.source.clone(), line: line.line, label, operation, operand }
}

fn strip_comment(line: &str) -> &str {
    let mut quote = None;
    for (index, character) in line.char_indices() {
        match character {
            '\'' | '"' if quote == Some(character) => quote = None,
            '\'' | '"' if quote.is_none() => quote = Some(character),
            ';' if quote.is_none() => return &line[..index],
            _ => {}
        }
    }
    line
}

fn error(diagnostics: &mut Vec<Diagnostic>, line: &SourceLine, message: impl Into<String>) {
    diagnostics.push(Diagnostic {
        source: line.source.clone(),
        line: line.line,
        column: 1,
        message: message.into(),
    });
}

fn statement_error(
    diagnostics: &mut Vec<Diagnostic>,
    statement: &Statement,
    message: impl Into<String>,
) {
    diagnostics.push(Diagnostic {
        source: statement.source.clone(),
        line: statement.line,
        column: 1,
        message: message.into(),
    });
}

fn plan_program(
    statements: &[Statement],
    diagnostics: &mut Vec<Diagnostic>,
) -> (Vec<PlannedLine>, BTreeMap<String, i32>) {
    let mut plan = Vec::new();
    let mut symbols = BTreeMap::new();
    let mut address = 0u16;
    for statement in statements {
        if let Some(label) = &statement.label
            && !matches!(statement.operation.as_deref(), Some("EQU" | "EQ"))
            && symbols.insert(label.clone(), i32::from(address)).is_some()
        {
            statement_error(diagnostics, statement, format!("duplicate symbol {label}"));
        }

        let mut planned =
            PlannedLine { statement: statement.clone(), address, size: 0, kind: PlanKind::Empty };
        let Some(operation) = statement.operation.as_deref() else {
            plan.push(planned);
            continue;
        };
        match operation {
            "EQU" | "EQ" => {
                let Some(label) = &statement.label else {
                    statement_error(diagnostics, statement, "EQU requires a label");
                    plan.push(planned);
                    continue;
                };
                match evaluate(&statement.operand, &symbols, address) {
                    Ok(Some(value)) if (0..=0xffff).contains(&value) => {
                        if symbols.insert(label.clone(), value).is_some() {
                            statement_error(
                                diagnostics,
                                statement,
                                format!("duplicate symbol {label}"),
                            );
                        }
                    }
                    Ok(Some(_)) => {
                        statement_error(diagnostics, statement, "EQU value is outside 0..65535")
                    }
                    Ok(None) => statement_error(
                        diagnostics,
                        statement,
                        "EQU expression has an unresolved forward reference",
                    ),
                    Err(message) => statement_error(diagnostics, statement, message),
                }
            }
            "ORG" => match evaluate(&statement.operand, &symbols, address) {
                Ok(Some(value)) if (0..=0xffff).contains(&value) => {
                    address = value as u16;
                    planned.address = address;
                    planned.kind = PlanKind::Origin;
                }
                Ok(Some(_)) => {
                    statement_error(diagnostics, statement, "ORG value is outside 0..65535")
                }
                Ok(None) => {
                    statement_error(diagnostics, statement, "ORG requires a resolved expression")
                }
                Err(message) => statement_error(diagnostics, statement, message),
            },
            "DFB" | "DB" | "BYTE" => {
                let values = split_arguments(&statement.operand);
                planned.size = values
                    .iter()
                    .map(|value| string_literal(value).map_or(1, |bytes| bytes.len()))
                    .sum();
                planned.kind = PlanKind::Bytes(values);
            }
            "DW" | "DA" | "WORD" => {
                let values = split_arguments(&statement.operand);
                planned.size = values.len() * 2;
                planned.kind = PlanKind::Words(values);
            }
            "ASC" | "TEXT" => {
                let values = split_arguments(&statement.operand);
                planned.size = values
                    .iter()
                    .map(|value| string_literal(value).map_or(1, |bytes| bytes.len()))
                    .sum();
                planned.kind = PlanKind::Ascii(values);
            }
            "HEX" => match parse_hex(&statement.operand) {
                Ok(bytes) => {
                    planned.size = bytes.len();
                    planned.kind = PlanKind::Hex(bytes);
                }
                Err(message) => statement_error(diagnostics, statement, message),
            },
            "DS" => match evaluate(&statement.operand, &symbols, address) {
                Ok(Some(value)) if (0..=0xffff).contains(&value) => {
                    planned.size = value as usize;
                    planned.kind = PlanKind::Reserve(statement.operand.clone());
                }
                Ok(Some(_)) => {
                    statement_error(diagnostics, statement, "DS size is outside 0..65535")
                }
                Ok(None) => {
                    statement_error(diagnostics, statement, "DS requires a resolved expression")
                }
                Err(message) => statement_error(diagnostics, statement, message),
            },
            "END" => {
                plan.push(planned);
                break;
            }
            mnemonic if is_mnemonic(mnemonic) => {
                match plan_instruction(mnemonic, &statement.operand, &symbols, address) {
                    Ok((mode, expression)) => {
                        planned.size = mode_size(mode);
                        planned.kind = PlanKind::Instruction {
                            mnemonic: mnemonic.to_owned(),
                            mode,
                            expression,
                        };
                    }
                    Err(message) => statement_error(diagnostics, statement, message),
                }
            }
            "MAC" | "EOM" | "PMC" | "<<<" | ">>>" => {
                statement_error(
                    diagnostics,
                    statement,
                    "macro directive was not resolved during expansion",
                );
            }
            other => statement_error(diagnostics, statement, format!("unknown operation {other}")),
        }
        if planned.size > usize::from(u16::MAX - address) + 1 {
            statement_error(diagnostics, statement, "output exceeds 64 KiB address space");
        } else {
            address = address.wrapping_add(planned.size as u16);
        }
        plan.push(planned);
    }
    (plan, symbols)
}

fn emit_program(
    plan: &[PlannedLine],
    symbols: BTreeMap<String, i32>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<AssembledProgram, Vec<Diagnostic>> {
    let origin = plan.iter().find(|line| line.size > 0).map_or(0, |line| line.address);
    let mut cursor = origin;
    let mut bytes = Vec::new();
    for line in plan {
        if matches!(line.kind, PlanKind::Origin | PlanKind::Empty) {
            continue;
        }
        if line.address < cursor {
            statement_error(diagnostics, &line.statement, "ORG moves backward over emitted output");
            continue;
        }
        bytes.resize(bytes.len() + usize::from(line.address - cursor), 0);
        cursor = line.address;
        let before = bytes.len();
        emit_line(line, &symbols, &mut bytes, diagnostics);
        cursor = cursor.wrapping_add((bytes.len() - before) as u16);
    }
    let public_symbols = symbols
        .into_iter()
        .filter_map(|(name, value)| u16::try_from(value).ok().map(|value| (name, value)))
        .collect();
    Ok(AssembledProgram { origin, bytes, symbols: public_symbols })
}

fn emit_line(
    line: &PlannedLine,
    symbols: &BTreeMap<String, i32>,
    output: &mut Vec<u8>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match &line.kind {
        PlanKind::Instruction { mnemonic, mode, expression } => {
            let Some(opcode) = opcode(mnemonic, *mode) else {
                statement_error(
                    diagnostics,
                    &line.statement,
                    format!("{mnemonic} does not support {}", mode_name(*mode)),
                );
                return;
            };
            output.push(opcode);
            let Some(expression) = expression else { return };
            let value = match evaluate(expression, symbols, line.address) {
                Ok(Some(value)) => value,
                Ok(None) => {
                    statement_error(
                        diagnostics,
                        &line.statement,
                        format!("unresolved expression {expression}"),
                    );
                    output.resize(output.len() + mode_size(*mode) - 1, 0);
                    return;
                }
                Err(message) => {
                    statement_error(diagnostics, &line.statement, message);
                    output.resize(output.len() + mode_size(*mode) - 1, 0);
                    return;
                }
            };
            match mode {
                Mode::Relative => {
                    let offset = value - (i32::from(line.address) + 2);
                    if !(-128..=127).contains(&offset) {
                        statement_error(
                            diagnostics,
                            &line.statement,
                            "branch target is out of range",
                        );
                        output.push(0);
                    } else {
                        output.push(offset as i8 as u8);
                    }
                }
                Mode::Immediate
                | Mode::ZeroPage
                | Mode::ZeroPageX
                | Mode::ZeroPageY
                | Mode::IndirectX
                | Mode::IndirectY => {
                    if !(-128..=255).contains(&value) {
                        statement_error(
                            diagnostics,
                            &line.statement,
                            "8-bit operand is out of range",
                        );
                    }
                    output.push(value as u8);
                }
                Mode::Absolute | Mode::AbsoluteX | Mode::AbsoluteY | Mode::Indirect => {
                    if !(0..=0xffff).contains(&value) {
                        statement_error(
                            diagnostics,
                            &line.statement,
                            "16-bit operand is out of range",
                        );
                    }
                    output.extend_from_slice(&(value as u16).to_le_bytes());
                }
                Mode::Implied | Mode::Accumulator => {}
            }
        }
        PlanKind::Bytes(values) | PlanKind::Ascii(values) => {
            for value in values {
                if let Some(string) = string_literal(value) {
                    output.extend(string);
                } else {
                    emit_byte_expression(value, line, symbols, output, diagnostics);
                }
            }
        }
        PlanKind::Words(values) => {
            for value in values {
                match evaluate(value, symbols, line.address) {
                    Ok(Some(value)) if (0..=0xffff).contains(&value) => {
                        output.extend_from_slice(&(value as u16).to_le_bytes());
                    }
                    Ok(Some(_)) => {
                        statement_error(diagnostics, &line.statement, "word value is out of range");
                        output.extend_from_slice(&[0, 0]);
                    }
                    Ok(None) => {
                        statement_error(
                            diagnostics,
                            &line.statement,
                            format!("unresolved expression {value}"),
                        );
                        output.extend_from_slice(&[0, 0]);
                    }
                    Err(message) => {
                        statement_error(diagnostics, &line.statement, message);
                        output.extend_from_slice(&[0, 0]);
                    }
                }
            }
        }
        PlanKind::Hex(bytes) => output.extend_from_slice(bytes),
        PlanKind::Reserve(expression) => match evaluate(expression, symbols, line.address) {
            Ok(Some(value)) if value >= 0 => output.resize(output.len() + value as usize, 0),
            _ => {
                statement_error(diagnostics, &line.statement, "invalid DS size");
                output.resize(output.len() + line.size, 0);
            }
        },
        PlanKind::Empty | PlanKind::Origin => {}
    }
}

fn emit_byte_expression(
    expression: &str,
    line: &PlannedLine,
    symbols: &BTreeMap<String, i32>,
    output: &mut Vec<u8>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match evaluate(expression, symbols, line.address) {
        Ok(Some(value)) if (-128..=255).contains(&value) => output.push(value as u8),
        Ok(Some(_)) => {
            statement_error(diagnostics, &line.statement, "byte value is out of range");
            output.push(0);
        }
        Ok(None) => {
            statement_error(
                diagnostics,
                &line.statement,
                format!("unresolved expression {expression}"),
            );
            output.push(0);
        }
        Err(message) => {
            statement_error(diagnostics, &line.statement, message);
            output.push(0);
        }
    }
}

fn plan_instruction(
    mnemonic: &str,
    operand: &str,
    symbols: &BTreeMap<String, i32>,
    address: u16,
) -> Result<(Mode, Option<String>), String> {
    let operand = operand.trim();
    if operand.is_empty() {
        return opcode(mnemonic, Mode::Implied)
            .map(|_| (Mode::Implied, None))
            .ok_or_else(|| format!("{mnemonic} requires an operand"));
    }
    if operand.eq_ignore_ascii_case("A") {
        return opcode(mnemonic, Mode::Accumulator)
            .map(|_| (Mode::Accumulator, None))
            .ok_or_else(|| format!("{mnemonic} does not support accumulator mode"));
    }
    if is_branch(mnemonic) {
        return Ok((Mode::Relative, Some(operand.to_owned())));
    }
    if let Some(expression) = operand.strip_prefix('#') {
        return choose_mode(mnemonic, Mode::Immediate, expression);
    }
    let upper = operand.to_ascii_uppercase();
    if upper.starts_with('(') && upper.ends_with(",X)") {
        return choose_mode(mnemonic, Mode::IndirectX, operand[1..operand.len() - 3].trim());
    }
    if upper.starts_with('(') && upper.ends_with("),Y") {
        return choose_mode(mnemonic, Mode::IndirectY, operand[1..operand.len() - 3].trim());
    }
    if upper.starts_with('(') && upper.ends_with(')') {
        return choose_mode(mnemonic, Mode::Indirect, operand[1..operand.len() - 1].trim());
    }
    let (expression, wide_mode, narrow_mode) = if upper.ends_with(",X") {
        (operand[..operand.len() - 2].trim(), Mode::AbsoluteX, Mode::ZeroPageX)
    } else if upper.ends_with(",Y") {
        (operand[..operand.len() - 2].trim(), Mode::AbsoluteY, Mode::ZeroPageY)
    } else {
        (operand, Mode::Absolute, Mode::ZeroPage)
    };
    let fits_zero_page = evaluate(expression, symbols, address)
        .ok()
        .flatten()
        .is_some_and(|value| (0..=255).contains(&value));
    if fits_zero_page && opcode(mnemonic, narrow_mode).is_some() {
        choose_mode(mnemonic, narrow_mode, expression)
    } else {
        choose_mode(mnemonic, wide_mode, expression)
    }
}

fn choose_mode(
    mnemonic: &str,
    mode: Mode,
    expression: &str,
) -> Result<(Mode, Option<String>), String> {
    opcode(mnemonic, mode)
        .map(|_| (mode, Some(expression.trim().to_owned())))
        .ok_or_else(|| format!("{mnemonic} does not support {}", mode_name(mode)))
}

const fn mode_size(mode: Mode) -> usize {
    match mode {
        Mode::Implied | Mode::Accumulator => 1,
        Mode::Immediate
        | Mode::ZeroPage
        | Mode::ZeroPageX
        | Mode::ZeroPageY
        | Mode::IndirectX
        | Mode::IndirectY
        | Mode::Relative => 2,
        Mode::Absolute | Mode::AbsoluteX | Mode::AbsoluteY | Mode::Indirect => 3,
    }
}

const fn mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::Implied => "implied mode",
        Mode::Accumulator => "accumulator mode",
        Mode::Immediate => "immediate mode",
        Mode::ZeroPage => "zero-page mode",
        Mode::ZeroPageX => "zero-page,X mode",
        Mode::ZeroPageY => "zero-page,Y mode",
        Mode::Absolute => "absolute mode",
        Mode::AbsoluteX => "absolute,X mode",
        Mode::AbsoluteY => "absolute,Y mode",
        Mode::Indirect => "indirect mode",
        Mode::IndirectX => "(zero-page,X) mode",
        Mode::IndirectY => "(zero-page),Y mode",
        Mode::Relative => "relative mode",
    }
}

fn split_arguments(operand: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut start = 0;
    let mut quote = None;
    let mut depth = 0;
    for (index, character) in operand.char_indices() {
        match character {
            '\'' | '"' if quote == Some(character) => quote = None,
            '\'' | '"' if quote.is_none() => quote = Some(character),
            '(' if quote.is_none() => depth += 1,
            ')' if quote.is_none() => depth -= 1,
            ',' if quote.is_none() && depth == 0 => {
                values.push(operand[start..index].trim().to_owned());
                start = index + 1;
            }
            _ => {}
        }
    }
    if start < operand.len() || !operand.trim().is_empty() {
        values.push(operand[start..].trim().to_owned());
    }
    values
}

fn string_literal(value: &str) -> Option<Vec<u8>> {
    let value = value.trim();
    let quote = value.as_bytes().first().copied()?;
    if !matches!(quote, b'\'' | b'"')
        || value.as_bytes().last().copied() != Some(quote)
        || value.len() < 2
    {
        return None;
    }
    Some(value.as_bytes()[1..value.len() - 1].to_vec())
}

fn parse_hex(operand: &str) -> Result<Vec<u8>, String> {
    let digits = operand
        .chars()
        .filter(|character| !character.is_whitespace() && *character != ',')
        .collect::<String>();
    if digits.len() % 2 != 0 || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("HEX requires an even number of hexadecimal digits".to_owned());
    }
    (0..digits.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&digits[index..index + 2], 16)
                .map_err(|_| "invalid HEX data".to_owned())
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Token {
    Number(i32),
    Identifier(String),
    Plus,
    Minus,
    Multiply,
    Divide,
    And,
    Or,
    Xor,
    Low,
    High,
    LeftParen,
    RightParen,
    ShiftLeft,
    ShiftRight,
    Not,
}

fn evaluate(
    expression: &str,
    symbols: &BTreeMap<String, i32>,
    address: u16,
) -> Result<Option<i32>, String> {
    let tokens = tokenize_expression(expression, address)?;
    if tokens.is_empty() {
        return Err("expression is required".to_owned());
    }
    let mut parser = ExpressionParser { tokens: &tokens, index: 0, symbols };
    let value = parser.parse_binary(0)?;
    if parser.index != tokens.len() {
        return Err("unexpected token in expression".to_owned());
    }
    Ok(value)
}

fn tokenize_expression(expression: &str, address: u16) -> Result<Vec<Token>, String> {
    let bytes = expression.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            byte if byte.is_ascii_whitespace() => index += 1,
            b'$' => {
                let start = index + 1;
                index = start;
                while index < bytes.len() && bytes[index].is_ascii_hexdigit() {
                    index += 1;
                }
                if start == index {
                    return Err("hex literal requires digits after $".to_owned());
                }
                let value = i32::from_str_radix(&expression[start..index], 16)
                    .map_err(|_| "hex literal is out of range".to_owned())?;
                tokens.push(Token::Number(value));
            }
            b'%' => {
                let start = index + 1;
                index = start;
                while index < bytes.len() && matches!(bytes[index], b'0' | b'1') {
                    index += 1;
                }
                if start == index {
                    return Err("binary literal requires digits after %".to_owned());
                }
                let value = i32::from_str_radix(&expression[start..index], 2)
                    .map_err(|_| "binary literal is out of range".to_owned())?;
                tokens.push(Token::Number(value));
            }
            b'0' if bytes.get(index + 1).is_some_and(|byte| matches!(byte, b'x' | b'X')) => {
                let start = index + 2;
                index = start;
                while index < bytes.len() && bytes[index].is_ascii_hexdigit() {
                    index += 1;
                }
                if start == index {
                    return Err("hex literal requires digits after 0x".to_owned());
                }
                let value = i32::from_str_radix(&expression[start..index], 16)
                    .map_err(|_| "hex literal is out of range".to_owned())?;
                tokens.push(Token::Number(value));
            }
            byte if byte.is_ascii_digit() => {
                let start = index;
                while index < bytes.len() && bytes[index].is_ascii_digit() {
                    index += 1;
                }
                let value = expression[start..index]
                    .parse::<i32>()
                    .map_err(|_| "decimal literal is out of range".to_owned())?;
                tokens.push(Token::Number(value));
            }
            b'\'' if index + 2 < bytes.len() && bytes[index + 2] == b'\'' => {
                tokens.push(Token::Number(i32::from(bytes[index + 1])));
                index += 3;
            }
            byte if byte.is_ascii_alphabetic() || matches!(byte, b'_' | b'.' | b']') => {
                let start = index;
                while index < bytes.len()
                    && (bytes[index].is_ascii_alphanumeric()
                        || matches!(bytes[index], b'_' | b'.' | b']'))
                {
                    index += 1;
                }
                tokens.push(Token::Identifier(expression[start..index].to_ascii_uppercase()));
            }
            b'+' => {
                tokens.push(Token::Plus);
                index += 1;
            }
            b'-' => {
                tokens.push(Token::Minus);
                index += 1;
            }
            b'*' => {
                let prefix = tokens.last().is_none_or(|token| {
                    matches!(
                        token,
                        Token::Plus
                            | Token::Minus
                            | Token::Multiply
                            | Token::Divide
                            | Token::And
                            | Token::Or
                            | Token::Xor
                            | Token::Low
                            | Token::High
                            | Token::LeftParen
                            | Token::ShiftLeft
                            | Token::ShiftRight
                            | Token::Not
                    )
                });
                if prefix {
                    tokens.push(Token::Number(i32::from(address)));
                } else {
                    tokens.push(Token::Multiply);
                }
                index += 1;
            }
            b'/' => {
                tokens.push(Token::Divide);
                index += 1;
            }
            b'&' => {
                tokens.push(Token::And);
                index += 1;
            }
            b'|' => {
                tokens.push(Token::Or);
                index += 1;
            }
            b'^' => {
                tokens.push(Token::Xor);
                index += 1;
            }
            b'~' => {
                tokens.push(Token::Not);
                index += 1;
            }
            b'(' => {
                tokens.push(Token::LeftParen);
                index += 1;
            }
            b')' => {
                tokens.push(Token::RightParen);
                index += 1;
            }
            b'<' if bytes.get(index + 1) == Some(&b'<') => {
                tokens.push(Token::ShiftLeft);
                index += 2;
            }
            b'>' if bytes.get(index + 1) == Some(&b'>') => {
                tokens.push(Token::ShiftRight);
                index += 2;
            }
            b'<' => {
                tokens.push(Token::Low);
                index += 1;
            }
            b'>' => {
                tokens.push(Token::High);
                index += 1;
            }
            other => return Err(format!("invalid expression character {}", char::from(other))),
        }
    }
    Ok(tokens)
}

struct ExpressionParser<'a> {
    tokens: &'a [Token],
    index: usize,
    symbols: &'a BTreeMap<String, i32>,
}

impl ExpressionParser<'_> {
    fn parse_binary(&mut self, minimum_precedence: u8) -> Result<Option<i32>, String> {
        let mut left = self.parse_unary()?;
        loop {
            let Some((precedence, operation)) =
                self.tokens.get(self.index).and_then(binary_operator)
            else {
                break;
            };
            if precedence < minimum_precedence {
                break;
            }
            self.index += 1;
            let right = self.parse_binary(precedence + 1)?;
            left = match (left, right) {
                (Some(left), Some(right)) => Some(apply_binary(operation, left, right)?),
                _ => None,
            };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Option<i32>, String> {
        let Some(token) = self.tokens.get(self.index) else {
            return Err("incomplete expression".to_owned());
        };
        match token {
            Token::Plus => {
                self.index += 1;
                self.parse_unary()
            }
            Token::Minus => {
                self.index += 1;
                Ok(self.parse_unary()?.map(|value| -value))
            }
            Token::Not => {
                self.index += 1;
                Ok(self.parse_unary()?.map(|value| !value))
            }
            Token::Low => {
                self.index += 1;
                Ok(self.parse_unary()?.map(|value| value & 0xff))
            }
            Token::High => {
                self.index += 1;
                Ok(self.parse_unary()?.map(|value| (value >> 8) & 0xff))
            }
            Token::Number(value) => {
                self.index += 1;
                Ok(Some(*value))
            }
            Token::Identifier(name) => {
                self.index += 1;
                Ok(self.symbols.get(name).copied())
            }
            Token::LeftParen => {
                self.index += 1;
                let value = self.parse_binary(0)?;
                if self.tokens.get(self.index) != Some(&Token::RightParen) {
                    return Err("missing closing parenthesis".to_owned());
                }
                self.index += 1;
                Ok(value)
            }
            _ => Err("expected a value".to_owned()),
        }
    }
}

fn binary_operator(token: &Token) -> Option<(u8, &Token)> {
    let precedence = match token {
        Token::Or => 1,
        Token::Xor => 2,
        Token::And => 3,
        Token::ShiftLeft | Token::ShiftRight => 4,
        Token::Plus | Token::Minus => 5,
        Token::Multiply | Token::Divide => 6,
        _ => return None,
    };
    Some((precedence, token))
}

fn apply_binary(operation: &Token, left: i32, right: i32) -> Result<i32, String> {
    match operation {
        Token::Plus => left.checked_add(right).ok_or_else(|| "expression overflow".to_owned()),
        Token::Minus => left.checked_sub(right).ok_or_else(|| "expression overflow".to_owned()),
        Token::Multiply => left.checked_mul(right).ok_or_else(|| "expression overflow".to_owned()),
        Token::Divide if right == 0 => Err("division by zero".to_owned()),
        Token::Divide => left.checked_div(right).ok_or_else(|| "expression overflow".to_owned()),
        Token::And => Ok(left & right),
        Token::Or => Ok(left | right),
        Token::Xor => Ok(left ^ right),
        Token::ShiftLeft => Ok(left.wrapping_shl(right as u32)),
        Token::ShiftRight => Ok(left.wrapping_shr(right as u32)),
        _ => unreachable!("only binary operators are passed here"),
    }
}

fn is_known_operation(operation: &str) -> bool {
    is_mnemonic(operation)
        || matches!(
            operation,
            "ORG"
                | "EQU"
                | "EQ"
                | "DS"
                | "DFB"
                | "DB"
                | "BYTE"
                | "DW"
                | "DA"
                | "WORD"
                | "ASC"
                | "TEXT"
                | "HEX"
                | "PUT"
                | "USE"
                | "INCLUDE"
                | "MAC"
                | "EOM"
                | "PMC"
                | "<<<"
                | ">>>"
                | "END"
        )
}

fn is_mnemonic(mnemonic: &str) -> bool {
    matches!(
        mnemonic,
        "ADC"
            | "AND"
            | "ASL"
            | "BCC"
            | "BCS"
            | "BEQ"
            | "BIT"
            | "BMI"
            | "BNE"
            | "BPL"
            | "BRK"
            | "BVC"
            | "BVS"
            | "CLC"
            | "CLD"
            | "CLI"
            | "CLV"
            | "CMP"
            | "CPX"
            | "CPY"
            | "DEC"
            | "DEX"
            | "DEY"
            | "EOR"
            | "INC"
            | "INX"
            | "INY"
            | "JMP"
            | "JSR"
            | "LDA"
            | "LDX"
            | "LDY"
            | "LSR"
            | "NOP"
            | "ORA"
            | "PHA"
            | "PHP"
            | "PLA"
            | "PLP"
            | "ROL"
            | "ROR"
            | "RTI"
            | "RTS"
            | "SBC"
            | "SEC"
            | "SED"
            | "SEI"
            | "STA"
            | "STX"
            | "STY"
            | "TAX"
            | "TAY"
            | "TSX"
            | "TXA"
            | "TXS"
            | "TYA"
    )
}

fn is_branch(mnemonic: &str) -> bool {
    matches!(mnemonic, "BCC" | "BCS" | "BEQ" | "BMI" | "BNE" | "BPL" | "BVC" | "BVS")
}

fn opcode(mnemonic: &str, mode: Mode) -> Option<u8> {
    use Mode::{
        Absolute as Abs, AbsoluteX as AbsX, AbsoluteY as AbsY, Accumulator as Acc,
        Immediate as Imm, Implied as Imp, Indirect as Ind, IndirectX as IndX, IndirectY as IndY,
        Relative as Rel, ZeroPage as Zp, ZeroPageX as ZpX, ZeroPageY as ZpY,
    };
    let entries: &[(Mode, u8)] = match mnemonic {
        "ADC" => &[
            (Imm, 0x69),
            (Zp, 0x65),
            (ZpX, 0x75),
            (Abs, 0x6d),
            (AbsX, 0x7d),
            (AbsY, 0x79),
            (IndX, 0x61),
            (IndY, 0x71),
        ],
        "AND" => &[
            (Imm, 0x29),
            (Zp, 0x25),
            (ZpX, 0x35),
            (Abs, 0x2d),
            (AbsX, 0x3d),
            (AbsY, 0x39),
            (IndX, 0x21),
            (IndY, 0x31),
        ],
        "ASL" => &[(Acc, 0x0a), (Zp, 0x06), (ZpX, 0x16), (Abs, 0x0e), (AbsX, 0x1e)],
        "BCC" => &[(Rel, 0x90)],
        "BCS" => &[(Rel, 0xb0)],
        "BEQ" => &[(Rel, 0xf0)],
        "BIT" => &[(Zp, 0x24), (Abs, 0x2c)],
        "BMI" => &[(Rel, 0x30)],
        "BNE" => &[(Rel, 0xd0)],
        "BPL" => &[(Rel, 0x10)],
        "BRK" => &[(Imp, 0x00)],
        "BVC" => &[(Rel, 0x50)],
        "BVS" => &[(Rel, 0x70)],
        "CLC" => &[(Imp, 0x18)],
        "CLD" => &[(Imp, 0xd8)],
        "CLI" => &[(Imp, 0x58)],
        "CLV" => &[(Imp, 0xb8)],
        "CMP" => &[
            (Imm, 0xc9),
            (Zp, 0xc5),
            (ZpX, 0xd5),
            (Abs, 0xcd),
            (AbsX, 0xdd),
            (AbsY, 0xd9),
            (IndX, 0xc1),
            (IndY, 0xd1),
        ],
        "CPX" => &[(Imm, 0xe0), (Zp, 0xe4), (Abs, 0xec)],
        "CPY" => &[(Imm, 0xc0), (Zp, 0xc4), (Abs, 0xcc)],
        "DEC" => &[(Zp, 0xc6), (ZpX, 0xd6), (Abs, 0xce), (AbsX, 0xde)],
        "DEX" => &[(Imp, 0xca)],
        "DEY" => &[(Imp, 0x88)],
        "EOR" => &[
            (Imm, 0x49),
            (Zp, 0x45),
            (ZpX, 0x55),
            (Abs, 0x4d),
            (AbsX, 0x5d),
            (AbsY, 0x59),
            (IndX, 0x41),
            (IndY, 0x51),
        ],
        "INC" => &[(Zp, 0xe6), (ZpX, 0xf6), (Abs, 0xee), (AbsX, 0xfe)],
        "INX" => &[(Imp, 0xe8)],
        "INY" => &[(Imp, 0xc8)],
        "JMP" => &[(Abs, 0x4c), (Ind, 0x6c)],
        "JSR" => &[(Abs, 0x20)],
        "LDA" => &[
            (Imm, 0xa9),
            (Zp, 0xa5),
            (ZpX, 0xb5),
            (Abs, 0xad),
            (AbsX, 0xbd),
            (AbsY, 0xb9),
            (IndX, 0xa1),
            (IndY, 0xb1),
        ],
        "LDX" => &[(Imm, 0xa2), (Zp, 0xa6), (ZpY, 0xb6), (Abs, 0xae), (AbsY, 0xbe)],
        "LDY" => &[(Imm, 0xa0), (Zp, 0xa4), (ZpX, 0xb4), (Abs, 0xac), (AbsX, 0xbc)],
        "LSR" => &[(Acc, 0x4a), (Zp, 0x46), (ZpX, 0x56), (Abs, 0x4e), (AbsX, 0x5e)],
        "NOP" => &[(Imp, 0xea)],
        "ORA" => &[
            (Imm, 0x09),
            (Zp, 0x05),
            (ZpX, 0x15),
            (Abs, 0x0d),
            (AbsX, 0x1d),
            (AbsY, 0x19),
            (IndX, 0x01),
            (IndY, 0x11),
        ],
        "PHA" => &[(Imp, 0x48)],
        "PHP" => &[(Imp, 0x08)],
        "PLA" => &[(Imp, 0x68)],
        "PLP" => &[(Imp, 0x28)],
        "ROL" => &[(Acc, 0x2a), (Zp, 0x26), (ZpX, 0x36), (Abs, 0x2e), (AbsX, 0x3e)],
        "ROR" => &[(Acc, 0x6a), (Zp, 0x66), (ZpX, 0x76), (Abs, 0x6e), (AbsX, 0x7e)],
        "RTI" => &[(Imp, 0x40)],
        "RTS" => &[(Imp, 0x60)],
        "SBC" => &[
            (Imm, 0xe9),
            (Zp, 0xe5),
            (ZpX, 0xf5),
            (Abs, 0xed),
            (AbsX, 0xfd),
            (AbsY, 0xf9),
            (IndX, 0xe1),
            (IndY, 0xf1),
        ],
        "SEC" => &[(Imp, 0x38)],
        "SED" => &[(Imp, 0xf8)],
        "SEI" => &[(Imp, 0x78)],
        "STA" => &[
            (Zp, 0x85),
            (ZpX, 0x95),
            (Abs, 0x8d),
            (AbsX, 0x9d),
            (AbsY, 0x99),
            (IndX, 0x81),
            (IndY, 0x91),
        ],
        "STX" => &[(Zp, 0x86), (ZpY, 0x96), (Abs, 0x8e)],
        "STY" => &[(Zp, 0x84), (ZpX, 0x94), (Abs, 0x8c)],
        "TAX" => &[(Imp, 0xaa)],
        "TAY" => &[(Imp, 0xa8)],
        "TSX" => &[(Imp, 0xba)],
        "TXA" => &[(Imp, 0x8a)],
        "TXS" => &[(Imp, 0x9a)],
        "TYA" => &[(Imp, 0x98)],
        _ => return None,
    };
    entries.iter().find_map(|(entry_mode, opcode)| (*entry_mode == mode).then_some(*opcode))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assembles_labels_branches_data_and_addressing_modes() {
        let source = r#"
         ORG   $8000
ZP       EQU   $20
START    LDX   #$00
         LDA   ZP
         LDA   MESSAGE,X
         BEQ   DONE
         STA   $0200,X
         INX
         BNE   START
DONE     RTS
MESSAGE  ASC   "HI"
         DFB   0
"#;
        let program = assemble(source).unwrap();
        assert_eq!(program.origin, 0x8000);
        assert_eq!(
            program.bytes,
            [
                0xa2, 0x00, 0xa5, 0x20, 0xbd, 0x10, 0x80, 0xf0, 0x06, 0x9d, 0x00, 0x02, 0xe8, 0xd0,
                0xf1, 0x60, b'H', b'I', 0,
            ]
        );
        assert_eq!(program.symbols["MESSAGE"], 0x8010);
    }

    #[test]
    fn expands_merlin_macros_with_parameters() {
        let source = r#"
LOADIMM  MAC
         LDA   #]1
         STA   ]2
         EOM
         ORG   $1000
         PMC   LOADIMM;$42;$20
"#;
        let program = assemble(source).unwrap();
        assert_eq!(program.bytes, [0xa9, 0x42, 0x85, 0x20]);
    }

    #[test]
    fn expressions_support_low_high_arithmetic_and_current_address() {
        let source = r#"
         ORG   $1234
HERE     EQU   *
         DFB   <HERE,>HERE,2+3*4,%1010
         DW    HERE+1
"#;
        let program = assemble(source).unwrap();
        assert_eq!(program.bytes, [0x34, 0x12, 14, 10, 0x35, 0x12]);
    }

    #[test]
    fn includes_are_loaded_before_assembly() {
        let source = "         PUT   defs.inc\n         LDA   VALUE";
        let program = assemble_with_loader("main.asm", source, |path| {
            assert_eq!(path, "defs.inc");
            Ok("VALUE EQU $42".to_owned())
        })
        .unwrap();
        assert_eq!(program.bytes, [0xa5, 0x42]);
    }

    #[test]
    fn end_ignores_following_source() {
        let program = assemble(" ORG $8000\n LDA #1\n END\n BRK").unwrap();
        assert_eq!(program.bytes, [0xa9, 0x01]);
    }

    #[test]
    fn reports_source_line_for_syntax_and_branch_errors() {
        let source = "         ORG $8000\n         LDA #$100\n         BNE $9000";
        let diagnostics = assemble(source).unwrap_err();
        assert!(diagnostics.iter().any(|error| error.line == 2 && error.message.contains("8-bit")));
        assert!(diagnostics.iter().any(|error| error.line == 3 && error.message.contains("range")));
    }
}
