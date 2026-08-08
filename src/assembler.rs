//! Two-pass, Merlin-inspired macro assembler for the Fanticon NMOS 6502.

use std::collections::{BTreeMap, HashMap};

use crate::machine::{BANK_SIZE, FIXED_ROM_END, FIXED_ROM_START};

pub const FANTICON_INCLUDE_NAME: &str = "FANTICON.INC";
pub const FANTICON_INCLUDE_SOURCE: &str = include_str!("../code-assets/fanticon.inc");

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
    pub segments: Vec<AssembledSegment>,
    pub source_map: Vec<SourceMapEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceMapEntry {
    pub source: String,
    pub line: usize,
    pub address: u16,
    pub length: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssembledSegment {
    pub origin: u16,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SymbolSection {
    Fixed,
    Bank(u8),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CartridgeSymbol {
    pub address: u16,
    pub section: SymbolSection,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssembledCartridge {
    pub fixed_rom: [u8; 0x4000],
    pub rom_banks: Vec<[u8; 0x4000]>,
    pub symbols: BTreeMap<String, CartridgeSymbol>,
    pub source_map: Vec<CartridgeSourceMapEntry>,
    /// Bytes actually emitted vs. available in each section, in FIXED then
    /// bank order, for a "how much ROM is left" readout. Tracked from the
    /// same per-byte write masks that catch overlapping output, rather than
    /// inferred from the `$FF` erase fill, so a bank that legitimately emits
    /// `$FF` data is not miscounted as free space.
    pub bank_usage: Vec<BankUsage>,
}

/// How much of one cartridge section (the FIXED bank or one switchable
/// `BANK`) a build actually wrote.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BankUsage {
    pub section: SymbolSection,
    pub used: usize,
    pub capacity: usize,
}

impl BankUsage {
    pub fn free(&self) -> usize {
        self.capacity - self.used
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CartridgeSourceMapEntry {
    pub source: String,
    pub line: usize,
    pub address: u16,
    pub length: usize,
    pub section: SymbolSection,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CartridgeSection {
    Fixed,
    Bank(u8),
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
    parameters: Vec<MacroParameter>,
}

#[derive(Clone)]
struct MacroParameter {
    name: String,
    default: Option<String>,
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
    let mut fanticon_included = false;
    let included = expand_includes(root, &mut loader, &mut diagnostics, 0, &mut fanticon_included);
    let expanded = expand_macros(included, &mut diagnostics);
    let expanded = expand_repeat_blocks(expanded, &mut diagnostics);
    let expanded = expand_conditionals(expanded, &mut diagnostics);
    let expanded = expand_procedure_scopes(expanded, &mut diagnostics);
    let expanded = expand_dummy_sections(expanded, &mut diagnostics);
    let statements = expanded.iter().map(parse_statement).collect::<Vec<_>>();
    let (plan, symbols) = plan_program(&statements, &mut diagnostics);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    emit_program(&plan, symbols, &mut diagnostics)
        .and_then(|program| if diagnostics.is_empty() { Ok(program) } else { Err(diagnostics) })
}

/// Assemble explicit `BANK` and `FIXED` sections into mapper-0 ROM images.
pub fn assemble_cartridge_with_loader<F>(
    source_name: &str,
    source: &str,
    mut loader: F,
) -> Result<AssembledCartridge, Vec<Diagnostic>>
where
    F: FnMut(&str) -> Result<String, String>,
{
    struct Block {
        section: CartridgeSection,
        statements: Vec<Statement>,
        labels: Vec<String>,
    }

    let mut diagnostics = Vec::new();
    let root = source_lines(source_name, source);
    let mut fanticon_included = false;
    let included = expand_includes(root, &mut loader, &mut diagnostics, 0, &mut fanticon_included);
    let expanded = expand_macros(included, &mut diagnostics);
    let expanded = expand_repeat_blocks(expanded, &mut diagnostics);
    let expanded = expand_conditionals(expanded, &mut diagnostics);
    let expanded = expand_procedure_scopes(expanded, &mut diagnostics);
    let expanded = expand_dummy_sections(expanded, &mut diagnostics);
    let statements = expanded.iter().map(parse_statement).collect::<Vec<_>>();
    let mut common = Vec::new();
    let mut blocks = Vec::<Block>::new();
    let mut current = None;
    for statement in statements {
        match statement.operation.as_deref() {
            Some("FIXED") => {
                if !statement.operand.trim().is_empty() {
                    statement_error(&mut diagnostics, &statement, "FIXED takes no operand");
                }
                blocks.push(Block {
                    section: CartridgeSection::Fixed,
                    statements: Vec::new(),
                    labels: Vec::new(),
                });
                current = Some(blocks.len() - 1);
            }
            Some("BANK") => {
                let bank = match evaluate(&statement.operand, &BTreeMap::new(), 0) {
                    Ok(Some(value)) if (0..=255).contains(&value) => value as u8,
                    _ => {
                        statement_error(
                            &mut diagnostics,
                            &statement,
                            "BANK requires a resolved value from 0 through 255",
                        );
                        0
                    }
                };
                blocks.push(Block {
                    section: CartridgeSection::Bank(bank),
                    statements: Vec::new(),
                    labels: Vec::new(),
                });
                current = Some(blocks.len() - 1);
            }
            Some("REQUIRE_FIXED") => {
                if !statement.operand.trim().is_empty() {
                    statement_error(&mut diagnostics, &statement, "REQUIRE_FIXED takes no operand");
                }
                match current.map(|index| blocks[index].section) {
                    Some(CartridgeSection::Fixed) => {}
                    Some(CartridgeSection::Bank(bank)) => statement_error(
                        &mut diagnostics,
                        &statement,
                        format!("REQUIRE_FIXED failed while BANK {bank} is selected"),
                    ),
                    None => statement_error(
                        &mut diagnostics,
                        &statement,
                        "REQUIRE_FIXED requires a selected FIXED section",
                    ),
                }
            }
            _ => {
                if let Some(index) = current {
                    blocks[index].statements.push(statement);
                } else if statement.operation.is_none()
                    || matches!(statement.operation.as_deref(), Some("EQU" | "EQ"))
                {
                    common.push(statement);
                } else {
                    statement_error(
                        &mut diagnostics,
                        &statement,
                        "select BANK or FIXED before emitting cartridge code",
                    );
                }
            }
        }
    }
    if blocks.is_empty() {
        diagnostics.push(Diagnostic {
            source: source_name.to_owned(),
            line: 1,
            column: 1,
            message: "cartridge source has no BANK or FIXED section".to_owned(),
        });
    }

    let mut owners = BTreeMap::<String, (usize, CartridgeSection)>::new();
    for (block_index, block) in blocks.iter_mut().enumerate() {
        for statement in &block.statements {
            if let Some(label) = &statement.label
                && !matches!(statement.operation.as_deref(), Some("EQU" | "EQ"))
            {
                if owners.insert(label.clone(), (block_index, block.section)).is_some() {
                    statement_error(
                        &mut diagnostics,
                        statement,
                        format!("duplicate symbol {label}"),
                    );
                }
                block.labels.push(label.clone());
            }
        }
    }

    for block in &mut blocks {
        for statement in &mut block.statements {
            statement.operand = replace_bankof(
                &statement.operand,
                &owners,
                &statement.source,
                statement.line,
                &mut diagnostics,
            );
        }
    }
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    // Resolve block addresses to a fixed point. External symbols begin at zero,
    // which can initially select a zero-page opcode; repeating the planning pass
    // lets the resulting instruction-size change propagate to later labels.
    let mut resolved = owners
        .iter()
        .map(|(name, (_, section))| (name.clone(), (0, *section)))
        .collect::<BTreeMap<_, _>>();
    let mut converged = false;
    for _ in 0..16 {
        let mut next = resolved.clone();
        let mut pass_diagnostics = Vec::new();
        for (index, block) in blocks.iter().enumerate() {
            let seed = resolved
                .iter()
                .filter(|(name, _)| owners.get(*name).map(|entry| entry.0) != Some(index))
                .map(|(name, (value, _))| (name.clone(), i32::from(*value)))
                .collect();
            let mut section_statements = common.clone();
            section_statements.extend(block.statements.clone());
            let (_, symbols) =
                plan_program_seeded(&section_statements, &mut pass_diagnostics, seed);
            for label in &block.labels {
                if let Some(value) = symbols.get(label).and_then(|value| u16::try_from(*value).ok())
                {
                    next.insert(label.clone(), (value, block.section));
                }
            }
        }
        if !pass_diagnostics.is_empty() {
            return Err(pass_diagnostics);
        }
        if next == resolved {
            converged = true;
            break;
        }
        resolved = next;
    }
    if !converged {
        return Err(vec![Diagnostic {
            source: source_name.to_owned(),
            line: 1,
            column: 1,
            message: "bank-aware symbol addresses did not converge".to_owned(),
        }]);
    }

    let mut fixed = [0xff; 0x4000];
    let mut fixed_written = [false; 0x4000];
    let highest_bank = blocks
        .iter()
        .filter_map(|block| match block.section {
            CartridgeSection::Bank(bank) => Some(bank),
            CartridgeSection::Fixed => None,
        })
        .max();
    let mut banks = vec![[0xff; 0x4000]; highest_bank.map_or(0, |bank| usize::from(bank) + 1)];
    let mut bank_written = vec![[false; 0x4000]; banks.len()];
    let mut public_symbols = BTreeMap::new();
    let mut source_map = Vec::new();

    for (index, block) in blocks.iter().enumerate() {
        let seed = resolved
            .iter()
            .filter(|(name, _)| owners.get(*name).map(|entry| entry.0) != Some(index))
            .map(|(name, (value, _))| (name.clone(), i32::from(*value)))
            .collect();
        let mut section_statements = common.clone();
        section_statements.extend(block.statements.clone());
        let (plan, symbols) = plan_program_seeded(&section_statements, &mut diagnostics, seed);
        let program = match emit_program(&plan, symbols, &mut diagnostics) {
            Ok(program) => program,
            Err(_) => continue,
        };
        for label in &block.labels {
            if let Some((address, section)) = resolved.get(label) {
                public_symbols.insert(
                    label.clone(),
                    CartridgeSymbol {
                        address: *address,
                        section: match section {
                            CartridgeSection::Fixed => SymbolSection::Fixed,
                            CartridgeSection::Bank(bank) => SymbolSection::Bank(*bank),
                        },
                    },
                );
            }
        }
        source_map.extend(program.source_map.iter().map(|entry| CartridgeSourceMapEntry {
            source: entry.source.clone(),
            line: entry.line,
            address: entry.address,
            length: entry.length,
            section: match block.section {
                CartridgeSection::Fixed => SymbolSection::Fixed,
                CartridgeSection::Bank(bank) => SymbolSection::Bank(bank),
            },
        }));
        for segment in program.segments {
            let (image, written, start, end) = match block.section {
                CartridgeSection::Fixed => {
                    (&mut fixed[..], &mut fixed_written[..], 0xc000u32, 0x10000u32)
                }
                CartridgeSection::Bank(bank) => (
                    &mut banks[bank as usize][..],
                    &mut bank_written[bank as usize][..],
                    0x8000u32,
                    0xc000u32,
                ),
            };
            let segment_start = u32::from(segment.origin);
            let segment_end = segment_start + segment.bytes.len() as u32;
            if segment_start < start || segment_end > end {
                diagnostics.push(Diagnostic {
                    source: source_name.to_owned(),
                    line: 1,
                    column: 1,
                    message: format!(
                        "ROM output ${segment_start:04X}-${:04X} crosses its section window",
                        segment_end.saturating_sub(1)
                    ),
                });
                continue;
            }
            if matches!(block.section, CartridgeSection::Fixed) && segment_start < 0xc100 {
                diagnostics.push(Diagnostic {
                    source: source_name.to_owned(),
                    line: 1,
                    column: 1,
                    message: "fixed output may not write the hidden $C000-$C0FF I/O page"
                        .to_owned(),
                });
                continue;
            }
            let offset = (segment_start - start) as usize;
            if written[offset..offset + segment.bytes.len()].iter().any(|value| *value) {
                diagnostics.push(Diagnostic {
                    source: source_name.to_owned(),
                    line: 1,
                    column: 1,
                    message: "ROM sections overlap previously emitted output".to_owned(),
                });
                continue;
            }
            image[offset..offset + segment.bytes.len()].copy_from_slice(&segment.bytes);
            written[offset..offset + segment.bytes.len()].fill(true);
        }
    }
    if !fixed_written[0x3ffa..0x4000].iter().all(|value| *value) {
        diagnostics.push(Diagnostic {
            source: source_name.to_owned(),
            line: 1,
            column: 1,
            message: "fixed ROM must explicitly provide NMI, RESET, and IRQ vectors at $FFFA-$FFFF"
                .to_owned(),
        });
    }
    let reset = u16::from_le_bytes([fixed[0x3ffc], fixed[0x3ffd]]);
    if !(reset <= 0x7fff || reset >= 0xc100) {
        diagnostics.push(Diagnostic {
            source: source_name.to_owned(),
            line: 1,
            column: 1,
            message: "RESET vector must point to main RAM or fixed ROM".to_owned(),
        });
    }
    if diagnostics.is_empty() {
        // The hidden $C000-$C0FF I/O page is carved out of FIXED above and can
        // never hold code, so it doesn't count as capacity either.
        let fixed_capacity = usize::from(FIXED_ROM_END - FIXED_ROM_START) + 1;
        let bank_usage = core::iter::once(BankUsage {
            section: SymbolSection::Fixed,
            used: fixed_written.iter().filter(|written| **written).count(),
            capacity: fixed_capacity,
        })
        .chain(bank_written.iter().enumerate().map(|(bank, written)| BankUsage {
            section: SymbolSection::Bank(bank as u8),
            used: written.iter().filter(|written| **written).count(),
            capacity: BANK_SIZE,
        }))
        .collect();
        Ok(AssembledCartridge {
            fixed_rom: fixed,
            rom_banks: banks,
            symbols: public_symbols,
            source_map,
            bank_usage,
        })
    } else {
        Err(diagnostics)
    }
}

fn replace_bankof(
    operand: &str,
    owners: &BTreeMap<String, (usize, CartridgeSection)>,
    source: &str,
    line: usize,
    diagnostics: &mut Vec<Diagnostic>,
) -> String {
    let mut result = operand.to_owned();
    let mut search = 0;
    loop {
        let upper = result.to_ascii_uppercase();
        let Some(relative) = upper[search..].find("BANKOF(") else { break };
        let start = search + relative;
        let name_start = start + 7;
        let Some(close_relative) = upper[name_start..].find(')') else {
            diagnostics.push(Diagnostic {
                source: source.to_owned(),
                line,
                column: 1,
                message: "BANKOF is missing a closing parenthesis".to_owned(),
            });
            break;
        };
        let close = name_start + close_relative;
        let name = upper[name_start..close].trim();
        let replacement = match owners.get(name) {
            Some((_, CartridgeSection::Bank(bank))) => Some(bank.to_string()),
            Some((_, CartridgeSection::Fixed)) | None => None,
        };
        let Some(replacement) = replacement else {
            diagnostics.push(Diagnostic {
                source: source.to_owned(),
                line,
                column: 1,
                message: format!("BANKOF requires a switchable-bank label: {name}"),
            });
            break;
        };
        result.replace_range(start..=close, &replacement);
        search = start + replacement.len();
    }
    result
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
    fanticon_included: &mut bool,
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
        if is_fanticon_include(path) {
            if !*fanticon_included {
                *fanticon_included = true;
                let nested = source_lines(FANTICON_INCLUDE_NAME, FANTICON_INCLUDE_SOURCE);
                output.extend(expand_includes(
                    nested,
                    loader,
                    diagnostics,
                    depth + 1,
                    fanticon_included,
                ));
            }
            continue;
        }
        match loader(path) {
            Ok(source) => {
                let nested = source_lines(path, &source);
                output.extend(expand_includes(
                    nested,
                    loader,
                    diagnostics,
                    depth + 1,
                    fanticon_included,
                ));
            }
            Err(message) => error(diagnostics, &line, format!("cannot include {path}: {message}")),
        }
    }
    output
}

fn is_fanticon_include(path: &str) -> bool {
    path.trim_start_matches(['/', '\\']).eq_ignore_ascii_case(FANTICON_INCLUDE_NAME)
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
        let parameters = match parse_macro_parameters(&statement.operand) {
            Ok(parameters) => parameters,
            Err(message) => {
                error(diagnostics, &lines[index], message);
                Vec::new()
            }
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
        if definitions.insert(name.clone(), MacroDefinition { body, parameters }).is_some() {
            error(diagnostics, &lines[index.saturating_sub(1)], format!("duplicate macro {name}"));
        }
    }

    let mut output = Vec::new();
    let mut expansion_id = 0u64;
    for line in ordinary {
        expand_macro_line(&line, &definitions, &mut output, diagnostics, &mut expansion_id, 0);
    }
    output
}

fn parse_macro_parameters(operand: &str) -> Result<Vec<MacroParameter>, String> {
    let operand = operand.trim();
    let operand = if operand.starts_with(';') {
        ""
    } else {
        operand.split_once(" ;").map_or(operand, |(parameters, _)| parameters.trim_end())
    };
    if operand.is_empty() {
        return Ok(Vec::new());
    }
    let mut parameters = Vec::new();
    let mut saw_default = false;
    for raw in split_macro_arguments(operand) {
        let (name, default) = raw
            .split_once('=')
            .map_or((raw.as_str(), None), |(name, value)| (name, Some(value.trim().to_owned())));
        let name = name.trim().to_ascii_uppercase();
        if name.is_empty()
            || !name.as_bytes()[0].is_ascii_alphabetic()
            || !name.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return Err(
                "macro parameter names must begin with a letter and use A-Z, 0-9, or _".to_owned()
            );
        }
        if parameters.iter().any(|parameter: &MacroParameter| parameter.name == name) {
            return Err(format!("duplicate macro parameter {name}"));
        }
        if default.as_deref() == Some("") {
            return Err(format!("macro parameter {name} has an empty default"));
        }
        if default.is_some() {
            saw_default = true;
        } else if saw_default {
            return Err("required macro parameters cannot follow defaulted parameters".to_owned());
        }
        parameters.push(MacroParameter { name, default });
    }
    if parameters.len() > 32 {
        return Err("macros support at most 32 named parameters".to_owned());
    }
    Ok(parameters)
}

fn expand_macro_line(
    line: &SourceLine,
    definitions: &HashMap<String, MacroDefinition>,
    output: &mut Vec<SourceLine>,
    diagnostics: &mut Vec<Diagnostic>,
    expansion_id: &mut u64,
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
                split.next().unwrap_or_default().to_owned(),
            )
        }
        Some(operation) if definitions.contains_key(operation) => {
            // `statement.operand` may already have been truncated at the
            // first unquoted `;` by parse_statement's generic comment
            // stripping, since it only recognizes `PMC`/`>>>` as macro
            // invocations, not arbitrary user-defined macro names. A macro
            // used directly still accepts the same semicolon-separated
            // argument list documented for `PMC`, so re-derive the
            // untruncated operand text from the raw line here.
            (operation.to_owned(), raw_macro_operand(&line.text, statement.label.is_some()))
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
    let arguments = split_macro_arguments(&arguments);
    let mut replacements = Vec::<(String, String)>::new();
    if definition.parameters.is_empty() {
        for parameter in 1..=8 {
            replacements.push((
                parameter.to_string(),
                arguments.get(parameter - 1).cloned().unwrap_or_default(),
            ));
        }
    } else {
        if arguments.len() > definition.parameters.len() {
            error(
                diagnostics,
                line,
                format!(
                    "macro {name} takes at most {} argument(s), got {}",
                    definition.parameters.len(),
                    arguments.len()
                ),
            );
            return;
        }
        for (index, parameter) in definition.parameters.iter().enumerate() {
            let value = arguments
                .get(index)
                .filter(|value| !value.is_empty())
                .cloned()
                .or_else(|| parameter.default.clone());
            let Some(value) = value else {
                error(
                    diagnostics,
                    line,
                    format!("macro {name} is missing argument {} ({})", index + 1, parameter.name),
                );
                return;
            };
            replacements.push((parameter.name.clone(), value.clone()));
            replacements.push(((index + 1).to_string(), value));
        }
    }
    replacements.sort_by(|left, right| right.0.len().cmp(&left.0.len()));
    let current_expansion = *expansion_id;
    *expansion_id = expansion_id.wrapping_add(1);
    for body_line in &definition.body {
        let text = rewrite_macro_text(&body_line.text, &replacements, current_expansion);
        let expanded = SourceLine { source: line.source.clone(), line: line.line, text };
        expand_macro_line(&expanded, definitions, output, diagnostics, expansion_id, depth + 1);
    }
}

fn rewrite_macro_text(text: &str, replacements: &[(String, String)], expansion_id: u64) -> String {
    let bytes = text.as_bytes();
    let mut output = String::with_capacity(text.len());
    let mut index = 0;
    let mut quote = None;
    while index < bytes.len() {
        let character = bytes[index] as char;
        if quote.is_none() && character == ';' {
            output.push_str(&text[index..]);
            break;
        }
        if matches!(character, '\'' | '"') {
            if quote == Some(character) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(character);
            }
            output.push(character);
            index += 1;
            continue;
        }
        if quote.is_none()
            && character == ']'
            && let Some((name, value)) = replacements.iter().find(|(name, _)| {
                text[index + 1..]
                    .get(..name.len())
                    .is_some_and(|candidate| candidate.eq_ignore_ascii_case(name))
                    && text
                        .as_bytes()
                        .get(index + 1 + name.len())
                        .is_none_or(|byte| !byte.is_ascii_alphanumeric() && *byte != b'_')
            })
        {
            output.push_str(value);
            index += name.len() + 1;
            continue;
        }
        if quote.is_none()
            && character == '@'
            && bytes.get(index + 1).is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
        {
            let start = index + 1;
            let mut end = start + 1;
            while bytes.get(end).is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_') {
                end += 1;
            }
            output.push_str(&format!("__M{expansion_id}_{}", &text[start..end]));
            index = end;
            continue;
        }
        output.push(character);
        index += 1;
    }
    output
}

/// Returns the raw, un-comment-stripped text following a macro invocation's
/// name (and label, if present), preserving semicolons so the caller can
/// split them as arguments rather than losing everything after the first
/// one to comment stripping.
fn raw_macro_operand(text: &str, has_label: bool) -> String {
    let mut remainder = text.trim_start();
    let fields_to_skip = if has_label { 2 } else { 1 };
    for _ in 0..fields_to_skip {
        remainder = match remainder.split_once(char::is_whitespace) {
            Some((_, rest)) => rest.trim_start(),
            None => "",
        };
    }
    remainder.trim_end().to_owned()
}

fn split_macro_arguments(arguments: &str) -> Vec<String> {
    let separator = if arguments.contains(';') { ';' } else { ',' };
    if arguments.trim().is_empty() {
        Vec::new()
    } else {
        arguments.split(separator).map(|argument| argument.trim().to_owned()).collect()
    }
}

fn expand_repeat_blocks(
    lines: Vec<SourceLine>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<SourceLine> {
    let mut symbols = BTreeMap::new();
    expand_repeat_sequence(&lines, &mut symbols, diagnostics)
}

fn expand_repeat_sequence(
    lines: &[SourceLine],
    symbols: &mut BTreeMap<String, i32>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<SourceLine> {
    let mut output = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let statement = parse_statement(&lines[index]);
        if matches!(statement.operation.as_deref(), Some("--^" | "ENDREP")) {
            error(diagnostics, &lines[index], "repeat terminator has no matching LUP or REPEAT");
            index += 1;
            continue;
        }
        if !matches!(statement.operation.as_deref(), Some("LUP" | "REPEAT")) {
            record_compile_constant(&statement, symbols, &lines[index], diagnostics);
            output.push(lines[index].clone());
            index += 1;
            continue;
        }

        let arguments = split_macro_arguments(&statement.operand);
        let expression = arguments.first().map_or("", String::as_str);
        let parameter = arguments
            .get(1)
            .map(|name| name.trim().trim_start_matches(']').to_ascii_uppercase())
            .unwrap_or_else(|| "I".to_owned());
        if parameter.is_empty()
            || !parameter.as_bytes()[0].is_ascii_alphabetic()
            || !parameter.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            error(diagnostics, &lines[index], "repeat parameter must be an identifier");
        }
        let count = match evaluate(expression, symbols, 0) {
            Ok(Some(value)) if (0..=65_535).contains(&value) => value as usize,
            Ok(Some(_)) => {
                error(diagnostics, &lines[index], "repeat count must be from 0 through 65535");
                0
            }
            Ok(None) => {
                error(diagnostics, &lines[index], "repeat count must be resolved when encountered");
                0
            }
            Err(message) => {
                error(diagnostics, &lines[index], message);
                0
            }
        };

        let body_start = index + 1;
        let mut depth = 1usize;
        index = body_start;
        while index < lines.len() && depth != 0 {
            match parse_statement(&lines[index]).operation.as_deref() {
                Some("LUP" | "REPEAT") => depth += 1,
                Some("--^" | "ENDREP") => depth -= 1,
                _ => {}
            }
            index += 1;
        }
        if depth != 0 {
            error(
                diagnostics,
                &lines[body_start.saturating_sub(1)],
                "repeat block is missing --^ or ENDREP",
            );
            break;
        }
        let body_end = index - 1;
        for iteration in 0..count {
            let replacements = [
                (parameter.clone(), iteration.to_string()),
                ("#".to_owned(), iteration.to_string()),
            ];
            let repeated = lines[body_start..body_end]
                .iter()
                .map(|line| SourceLine {
                    source: line.source.clone(),
                    line: line.line,
                    text: rewrite_repeat_text(&line.text, &replacements),
                })
                .collect::<Vec<_>>();
            output.extend(expand_repeat_sequence(&repeated, symbols, diagnostics));
        }
    }
    output
}

fn rewrite_repeat_text(text: &str, replacements: &[(String, String)]) -> String {
    let bytes = text.as_bytes();
    let mut output = String::with_capacity(text.len());
    let mut index = 0;
    let mut quote = None;
    while index < bytes.len() {
        let character = bytes[index] as char;
        if quote.is_none() && character == ';' {
            output.push_str(&text[index..]);
            break;
        }
        if matches!(character, '\'' | '"') {
            if quote == Some(character) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(character);
            }
            output.push(character);
            index += 1;
            continue;
        }
        if quote.is_none()
            && character == ']'
            && let Some((name, value)) = replacements.iter().find(|(name, _)| {
                text[index + 1..]
                    .get(..name.len())
                    .is_some_and(|candidate| candidate.eq_ignore_ascii_case(name))
                    && text
                        .as_bytes()
                        .get(index + 1 + name.len())
                        .is_none_or(|byte| !byte.is_ascii_alphanumeric() && *byte != b'_')
            })
        {
            output.push_str(value);
            index += name.len() + 1;
            continue;
        }
        output.push(character);
        index += 1;
    }
    output
}

fn record_compile_constant(
    statement: &Statement,
    symbols: &mut BTreeMap<String, i32>,
    line: &SourceLine,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !matches!(statement.operation.as_deref(), Some("EQU" | "EQ")) {
        return;
    }
    let Some(label) = &statement.label else { return };
    match evaluate(&statement.operand, symbols, 0) {
        Ok(Some(value)) => {
            symbols.insert(label.clone(), value);
        }
        Ok(None) => {}
        Err(message) => error(diagnostics, line, message),
    }
}

fn expand_conditionals(
    lines: Vec<SourceLine>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<SourceLine> {
    struct Conditional {
        parent_active: bool,
        condition: bool,
        else_seen: bool,
    }

    let mut output = Vec::new();
    let mut symbols = BTreeMap::new();
    let mut stack = Vec::<Conditional>::new();
    let mut active = true;
    for line in lines {
        let statement = parse_statement(&line);
        match statement.operation.as_deref() {
            Some("DO" | "IF") => {
                let condition = if active {
                    match evaluate(&statement.operand, &symbols, 0) {
                        Ok(Some(value)) => value != 0,
                        Ok(None) => {
                            error(
                                diagnostics,
                                &line,
                                "conditional expression must be resolved when encountered",
                            );
                            false
                        }
                        Err(message) => {
                            error(diagnostics, &line, message);
                            false
                        }
                    }
                } else {
                    false
                };
                stack.push(Conditional { parent_active: active, condition, else_seen: false });
                active &= condition;
            }
            Some("ELSE") => {
                let Some(frame) = stack.last_mut() else {
                    error(diagnostics, &line, "ELSE has no matching DO or IF");
                    continue;
                };
                if frame.else_seen {
                    error(diagnostics, &line, "conditional block contains more than one ELSE");
                }
                frame.else_seen = true;
                active = frame.parent_active && !frame.condition;
            }
            Some("FIN" | "ENDIF") => {
                let Some(frame) = stack.pop() else {
                    error(diagnostics, &line, "conditional terminator has no matching DO or IF");
                    continue;
                };
                active = frame.parent_active;
            }
            _ if active => {
                record_compile_constant(&statement, &mut symbols, &line, diagnostics);
                output.push(line);
            }
            _ => {}
        }
    }
    if !stack.is_empty() {
        let line = output.last().cloned().unwrap_or(SourceLine {
            source: "<memory>".to_owned(),
            line: 1,
            text: String::new(),
        });
        error(diagnostics, &line, "conditional block is missing FIN or ENDIF");
    }
    output
}

fn expand_procedure_scopes(
    lines: Vec<SourceLine>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<SourceLine> {
    let mut output = Vec::new();
    let mut procedure = None::<String>;
    for line in lines {
        let statement = parse_statement(&line);
        match statement.operation.as_deref() {
            Some("PROC") => {
                let Some(name) = statement.label else {
                    error(diagnostics, &line, "PROC requires a name in the label field");
                    continue;
                };
                if procedure.is_some() {
                    error(diagnostics, &line, "procedure scopes cannot nest");
                    continue;
                }
                procedure = Some(name.clone());
                output.push(SourceLine { source: line.source, line: line.line, text: name });
            }
            Some("ENDPROC") => {
                if procedure.take().is_none() {
                    error(diagnostics, &line, "ENDPROC has no matching PROC");
                }
            }
            _ => {
                let text = procedure.as_deref().map_or_else(
                    || line.text.clone(),
                    |name| rewrite_procedure_locals(&line.text, name),
                );
                output.push(SourceLine { text, ..line });
            }
        }
    }
    if let Some(name) = procedure {
        let line = output.last().cloned().unwrap_or(SourceLine {
            source: "<memory>".to_owned(),
            line: 1,
            text: String::new(),
        });
        error(diagnostics, &line, format!("procedure {name} is missing ENDPROC"));
    }
    output
}

fn rewrite_procedure_locals(text: &str, procedure: &str) -> String {
    let bytes = text.as_bytes();
    let mut output = String::with_capacity(text.len() + procedure.len());
    let mut index = 0;
    let mut quote = None;
    while index < bytes.len() {
        let character = bytes[index] as char;
        if quote.is_none() && character == ';' {
            output.push_str(&text[index..]);
            break;
        }
        if matches!(character, '\'' | '"') {
            if quote == Some(character) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(character);
            }
            output.push(character);
            index += 1;
            continue;
        }
        let previous_is_identifier = index
            .checked_sub(1)
            .and_then(|previous| bytes.get(previous))
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.'));
        if quote.is_none()
            && character == '.'
            && !previous_is_identifier
            && bytes.get(index + 1).is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
        {
            output.push_str(procedure);
        }
        output.push(character);
        index += 1;
    }
    output
}

fn expand_dummy_sections(
    lines: Vec<SourceLine>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<SourceLine> {
    struct DummySection {
        prefix: Option<String>,
        start: i32,
        offset: i32,
    }

    let mut output = Vec::new();
    let mut symbols = BTreeMap::new();
    let mut dummy = None::<DummySection>;
    for line in lines {
        let statement = parse_statement(&line);
        if let Some(section) = dummy.as_mut() {
            if statement.operation.as_deref() == Some("DEND") {
                if let Some(prefix) = &section.prefix {
                    let name = format!("{prefix}.SIZE");
                    let value = section.offset - section.start;
                    symbols.insert(name.clone(), value);
                    output.push(SourceLine {
                        source: line.source,
                        line: line.line,
                        text: format!("{name} EQU {value}"),
                    });
                }
                dummy = None;
                continue;
            }
            if statement.operation.is_none() {
                continue;
            }
            let Some(label) = statement.label else {
                error(diagnostics, &line, "DUM entries require a label");
                continue;
            };
            let name =
                section.prefix.as_ref().map_or(label.clone(), |prefix| format!("{prefix}.{label}"));
            match statement.operation.as_deref() {
                Some("DS") => {
                    let size = match evaluate(&statement.operand, &symbols, 0) {
                        Ok(Some(value)) if value >= 0 => value,
                        Ok(Some(_)) => {
                            error(diagnostics, &line, "DUM field size cannot be negative");
                            0
                        }
                        Ok(None) => {
                            error(diagnostics, &line, "DUM field size must be resolved");
                            0
                        }
                        Err(message) => {
                            error(diagnostics, &line, message);
                            0
                        }
                    };
                    symbols.insert(name.clone(), section.offset);
                    output.push(SourceLine {
                        source: line.source.clone(),
                        line: line.line,
                        text: format!("{name} EQU {}", section.offset),
                    });
                    section.offset = match section.offset.checked_add(size) {
                        Some(value) => value,
                        None => {
                            error(diagnostics, &line, "DUM layout overflows expression range");
                            section.offset
                        }
                    };
                }
                Some("EQU" | "EQ") => {
                    let value = match evaluate(&statement.operand, &symbols, 0) {
                        Ok(Some(value)) => value,
                        Ok(None) => {
                            error(diagnostics, &line, "DUM constant must be resolved");
                            0
                        }
                        Err(message) => {
                            error(diagnostics, &line, message);
                            0
                        }
                    };
                    symbols.insert(name.clone(), value);
                    output.push(SourceLine {
                        source: line.source,
                        line: line.line,
                        text: format!("{name} EQU {value}"),
                    });
                }
                _ => error(diagnostics, &line, "DUM supports labeled DS, EQU, and EQ entries"),
            }
            continue;
        }

        if statement.operation.as_deref() == Some("DEND") {
            error(diagnostics, &line, "DEND has no matching DUM");
            continue;
        }
        if statement.operation.as_deref() == Some("DUM") {
            let start = match evaluate(&statement.operand, &symbols, 0) {
                Ok(Some(value)) => value,
                Ok(None) => {
                    error(diagnostics, &line, "DUM origin must be resolved");
                    0
                }
                Err(message) => {
                    error(diagnostics, &line, message);
                    0
                }
            };
            dummy = Some(DummySection { prefix: statement.label, start, offset: start });
            continue;
        }
        record_compile_constant(&statement, &mut symbols, &line, diagnostics);
        output.push(line);
    }
    if let Some(section) = dummy {
        let line = output.last().cloned().unwrap_or(SourceLine {
            source: "<memory>".to_owned(),
            line: 1,
            text: String::new(),
        });
        let name = section.prefix.as_deref().unwrap_or("anonymous");
        error(diagnostics, &line, format!("DUM section {name} is missing DEND"));
    }
    output
}

fn parse_statement(line: &SourceLine) -> Statement {
    let preserves_semicolon_arguments = line.text.split_whitespace().take(2).any(|field| {
        matches!(field.to_ascii_uppercase().as_str(), "MAC" | "PMC" | ">>>" | "LUP" | "REPEAT")
    });
    let code =
        if preserves_semicolon_arguments { line.text.as_str() } else { strip_comment(&line.text) };
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
    plan_program_seeded(statements, diagnostics, BTreeMap::new())
}

fn plan_program_seeded(
    statements: &[Statement],
    diagnostics: &mut Vec<Diagnostic>,
    mut symbols: BTreeMap<String, i32>,
) -> (Vec<PlannedLine>, BTreeMap<String, i32>) {
    let mut plan = Vec::new();
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
            "REQUIRE_FIXED" => {
                if !statement.operand.trim().is_empty() {
                    statement_error(diagnostics, statement, "REQUIRE_FIXED takes no operand");
                }
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
    let mut segments = Vec::<AssembledSegment>::new();
    let mut source_map = Vec::new();
    let mut segment = None::<AssembledSegment>;
    for line in plan {
        if matches!(line.kind, PlanKind::Origin) {
            if let Some(previous) = segment.take()
                && !previous.bytes.is_empty()
            {
                segments.push(previous);
            }
            continue;
        }
        if matches!(line.kind, PlanKind::Empty) {
            continue;
        }
        if line.address < cursor {
            statement_error(diagnostics, &line.statement, "ORG moves backward over emitted output");
            continue;
        }
        bytes.resize(bytes.len() + usize::from(line.address - cursor), 0);
        cursor = line.address;
        let mut line_bytes = Vec::with_capacity(line.size);
        emit_line(line, &symbols, &mut line_bytes, diagnostics);
        if !line_bytes.is_empty() {
            source_map.push(SourceMapEntry {
                source: line.statement.source.clone(),
                line: line.statement.line,
                address: line.address,
                length: line_bytes.len(),
            });
        }
        bytes.extend_from_slice(&line_bytes);
        let active = segment
            .get_or_insert_with(|| AssembledSegment { origin: line.address, bytes: Vec::new() });
        active.bytes.extend_from_slice(&line_bytes);
        cursor = cursor.wrapping_add(line_bytes.len() as u16);
    }
    if let Some(previous) = segment
        && !previous.bytes.is_empty()
    {
        segments.push(previous);
    }
    let public_symbols = symbols
        .into_iter()
        .filter_map(|(name, value)| u16::try_from(value).ok().map(|value| (name, value)))
        .collect();
    Ok(AssembledProgram { origin, bytes, symbols: public_symbols, segments, source_map })
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
    Equal,
    NotEqual,
    LessOrEqual,
    GreaterOrEqual,
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
            b'=' if bytes.get(index + 1) == Some(&b'=') => {
                tokens.push(Token::Equal);
                index += 2;
            }
            b'!' if bytes.get(index + 1) == Some(&b'=') => {
                tokens.push(Token::NotEqual);
                index += 2;
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
            b'<' if bytes.get(index + 1) == Some(&b'=') => {
                tokens.push(Token::LessOrEqual);
                index += 2;
            }
            b'>' if bytes.get(index + 1) == Some(&b'>') => {
                tokens.push(Token::ShiftRight);
                index += 2;
            }
            b'>' if bytes.get(index + 1) == Some(&b'=') => {
                tokens.push(Token::GreaterOrEqual);
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
        Token::Equal
        | Token::NotEqual
        | Token::Low
        | Token::High
        | Token::LessOrEqual
        | Token::GreaterOrEqual => 1,
        Token::Or => 2,
        Token::Xor => 3,
        Token::And => 4,
        Token::ShiftLeft | Token::ShiftRight => 5,
        Token::Plus | Token::Minus => 6,
        Token::Multiply | Token::Divide => 7,
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
        Token::Equal => Ok(i32::from(left == right)),
        Token::NotEqual => Ok(i32::from(left != right)),
        Token::Low => Ok(i32::from(left < right)),
        Token::High => Ok(i32::from(left > right)),
        Token::LessOrEqual => Ok(i32::from(left <= right)),
        Token::GreaterOrEqual => Ok(i32::from(left >= right)),
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
                | "BANK"
                | "FIXED"
                | "REQUIRE_FIXED"
                | "MAC"
                | "EOM"
                | "PMC"
                | "<<<"
                | ">>>"
                | "DO"
                | "IF"
                | "ELSE"
                | "FIN"
                | "ENDIF"
                | "LUP"
                | "REPEAT"
                | "--^"
                | "ENDREP"
                | "PROC"
                | "ENDPROC"
                | "DUM"
                | "DEND"
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
            | "KIL"
            | "JAM"
            | "SLO"
            | "RLA"
            | "SRE"
            | "RRA"
            | "SAX"
            | "LAX"
            | "DCP"
            | "ISC"
            | "ISB"
            | "ANC"
            | "ALR"
            | "ARR"
            | "XAA"
            | "AXS"
            | "SBX"
            | "AHX"
            | "SHY"
            | "SHX"
            | "TAS"
            | "LAS"
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
        "NOP" => &[(Imp, 0xea), (Imm, 0x80), (Zp, 0x04), (ZpX, 0x14), (Abs, 0x0c), (AbsX, 0x1c)],
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
        "KIL" | "JAM" => &[(Imp, 0x02)],
        "SLO" => &[
            (IndX, 0x03),
            (Zp, 0x07),
            (Abs, 0x0f),
            (IndY, 0x13),
            (ZpX, 0x17),
            (AbsY, 0x1b),
            (AbsX, 0x1f),
        ],
        "RLA" => &[
            (IndX, 0x23),
            (Zp, 0x27),
            (Abs, 0x2f),
            (IndY, 0x33),
            (ZpX, 0x37),
            (AbsY, 0x3b),
            (AbsX, 0x3f),
        ],
        "SRE" => &[
            (IndX, 0x43),
            (Zp, 0x47),
            (Abs, 0x4f),
            (IndY, 0x53),
            (ZpX, 0x57),
            (AbsY, 0x5b),
            (AbsX, 0x5f),
        ],
        "RRA" => &[
            (IndX, 0x63),
            (Zp, 0x67),
            (Abs, 0x6f),
            (IndY, 0x73),
            (ZpX, 0x77),
            (AbsY, 0x7b),
            (AbsX, 0x7f),
        ],
        "SAX" => &[(IndX, 0x83), (Zp, 0x87), (Abs, 0x8f), (ZpY, 0x97)],
        "LAX" => &[
            (Imm, 0xab),
            (IndX, 0xa3),
            (Zp, 0xa7),
            (Abs, 0xaf),
            (IndY, 0xb3),
            (ZpY, 0xb7),
            (AbsY, 0xbf),
        ],
        "DCP" => &[
            (IndX, 0xc3),
            (Zp, 0xc7),
            (Abs, 0xcf),
            (IndY, 0xd3),
            (ZpX, 0xd7),
            (AbsY, 0xdb),
            (AbsX, 0xdf),
        ],
        "ISC" | "ISB" => &[
            (IndX, 0xe3),
            (Zp, 0xe7),
            (Abs, 0xef),
            (IndY, 0xf3),
            (ZpX, 0xf7),
            (AbsY, 0xfb),
            (AbsX, 0xff),
        ],
        "ANC" => &[(Imm, 0x0b)],
        "ALR" => &[(Imm, 0x4b)],
        "ARR" => &[(Imm, 0x6b)],
        "XAA" => &[(Imm, 0x8b)],
        "AXS" | "SBX" => &[(Imm, 0xcb)],
        "AHX" => &[(IndY, 0x93), (AbsY, 0x9f)],
        "SHY" => &[(AbsX, 0x9c)],
        "SHX" => &[(AbsY, 0x9e)],
        "TAS" => &[(AbsY, 0x9b)],
        "LAS" => &[(AbsY, 0xbb)],
        _ => return None,
    };
    entries.iter().find_map(|(entry_mode, opcode)| (*entry_mode == mode).then_some(*opcode))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_maps_preserve_file_line_address_length_and_cartridge_bank() {
        let program = assemble_with_loader(
            "main.asm",
            " ORG $8000\n PUT defs.inc\n LDA #VALUE\n RTS",
            |path| {
                (path == "defs.inc")
                    .then(|| "VALUE EQU 1".to_owned())
                    .ok_or_else(|| "not found".to_owned())
            },
        )
        .unwrap();
        assert_eq!(
            program.source_map,
            [
                SourceMapEntry {
                    source: "main.asm".to_owned(),
                    line: 3,
                    address: 0x8000,
                    length: 2,
                },
                SourceMapEntry {
                    source: "main.asm".to_owned(),
                    line: 4,
                    address: 0x8002,
                    length: 1,
                },
            ]
        );

        let cartridge = assemble_cartridge_with_loader(
            "main.asm",
            " BANK 3\n ORG $8000\nENTRY NOP\n FIXED\n ORG $C100\nRESET JMP RESET\nNMI RTI\nIRQ RTI\n ORG $FFFA\n DA NMI,RESET,IRQ",
            |_| Err("not found".to_owned()),
        )
        .unwrap();
        assert!(cartridge.source_map.iter().any(|entry| {
            entry.line == 3 && entry.address == 0x8000 && entry.section == SymbolSection::Bank(3)
        }));
        assert!(cartridge.source_map.iter().any(|entry| {
            entry.line == 6 && entry.address == 0xc100 && entry.section == SymbolSection::Fixed
        }));
    }

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
    fn expands_macro_invoked_directly_with_semicolon_arguments() {
        // A macro name used directly in the operation field (no `PMC`
        // prefix) must accept the same semicolon-separated argument list as
        // `PMC`, rather than losing every argument after the first `;` to
        // comment stripping.
        let source = r#"
LOADIMM  MAC
         LDA   #]1
         STA   ]2
         EOM
         ORG   $1000
         LOADIMM $42;$20
"#;
        let program = assemble(source).unwrap();
        assert_eq!(program.bytes, [0xa9, 0x42, 0x85, 0x20]);
    }

    #[test]
    fn named_macro_parameters_support_defaults_and_positional_aliases() {
        let source = r#"
STORE    MAC   VALUE;DEST=$20
         LDA   #]VALUE
         STA   ]DEST
         LDX   #]1
         EOM
         ORG   $1000
         STORE $42
         STORE $18;$30
"#;
        let program = assemble(source).unwrap();
        assert_eq!(
            program.bytes,
            [0xa9, 0x42, 0x85, 0x20, 0xa2, 0x42, 0xa9, 0x18, 0x85, 0x30, 0xa2, 0x18]
        );
    }

    #[test]
    fn macro_local_labels_are_unique_per_nested_expansion() {
        let source = r#"
WAIT     MAC   COUNT
         LDX   #]COUNT
@LOOP    DEX
         BNE   @LOOP
         EOM
TWICE    MAC   COUNT
         WAIT  ]COUNT
         WAIT  ]COUNT
         EOM
         ORG   $1000
         TWICE 2
         TWICE 3
"#;
        let program = assemble(source).unwrap();
        assert_eq!(
            program.bytes,
            [
                0xa2, 2, 0xca, 0xd0, 0xfd, 0xa2, 2, 0xca, 0xd0, 0xfd, 0xa2, 3, 0xca, 0xd0, 0xfd,
                0xa2, 3, 0xca, 0xd0, 0xfd
            ]
        );
    }

    #[test]
    fn compile_time_conditionals_support_nesting_aliases_and_comparisons() {
        let source = r#"
VERSION  EQU   2
         ORG   $1000
         IF    VERSION >= 2
         LDA   #1
         DO    VERSION == 3
         LDX   #0
         ELSE
         LDX   #2
         FIN
         ELSE
         LDA   #0
         ENDIF
"#;
        let program = assemble(source).unwrap();
        assert_eq!(program.bytes, [0xa9, 1, 0xa2, 2]);
    }

    #[test]
    fn repeat_blocks_support_named_indexes_and_nesting() {
        let source = r#"
         ORG   $1000
         REPEAT 3;ROW
         DFB   ]ROW
         LUP   2;COLUMN
         DFB   ]ROW*10+]COLUMN
         --^
         ENDREP
"#;
        let program = assemble(source).unwrap();
        assert_eq!(program.bytes, [0, 0, 1, 1, 10, 11, 2, 20, 21]);
    }

    #[test]
    fn procedure_scopes_make_dot_labels_local() {
        let source = r#"
         ORG   $1000
FIRST    PROC
.LOOP    DEX
         BNE   .LOOP
         RTS
         ENDPROC
SECOND   PROC
.LOOP    DEY
         BNE   .LOOP
         JSR   FIRST
         RTS
         ENDPROC
"#;
        let program = assemble(source).unwrap();
        assert_eq!(
            program.bytes,
            [0xca, 0xd0, 0xfd, 0x60, 0x88, 0xd0, 0xfd, 0x20, 0x00, 0x10, 0x60]
        );
        assert_eq!(program.symbols["FIRST.LOOP"], 0x1000);
        assert_eq!(program.symbols["SECOND.LOOP"], 0x1004);
    }

    #[test]
    fn dummy_sections_define_prefixed_layouts_without_emitting_bytes() {
        let source = r#"
PLAYER   DUM   0
X        DS    2
Y        DS    1
FLAGS    DS    1
         DEND
         ORG   $1000
HERO     DS    PLAYER.SIZE
         LDA   HERO+PLAYER.Y
"#;
        let program = assemble(source).unwrap();
        assert_eq!(program.bytes, [0, 0, 0, 0, 0xad, 0x02, 0x10]);
        assert_eq!(program.symbols["PLAYER.X"], 0);
        assert_eq!(program.symbols["PLAYER.Y"], 2);
        assert_eq!(program.symbols["PLAYER.FLAGS"], 3);
        assert_eq!(program.symbols["PLAYER.SIZE"], 4);
    }

    #[test]
    fn modern_macro_errors_report_bad_arguments_and_unclosed_blocks() {
        let missing =
            assemble("COPY MAC SOURCE;DEST\n NOP\n EOM\n ORG $1000\n COPY $20").unwrap_err();
        assert!(missing[0].message.contains("missing argument 2 (DEST)"));

        let unclosed = assemble(" ORG $1000\n IF 1\n NOP").unwrap_err();
        assert!(unclosed.iter().any(|diagnostic| diagnostic.message.contains("missing FIN")));

        let repeat = assemble(" ORG $1000\n REPEAT 2;I\n DFB ]I").unwrap_err();
        assert!(repeat.iter().any(|diagnostic| diagnostic.message.contains("missing --^")));

        let procedure = assemble(" ORG $1000\nBROKEN PROC\n RTS").unwrap_err();
        assert!(procedure.iter().any(|diagnostic| diagnostic.message.contains("missing ENDPROC")));

        let dummy = assemble("THING DUM 0\nFIELD DS 1").unwrap_err();
        assert!(dummy.iter().any(|diagnostic| diagnostic.message.contains("missing DEND")));
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
    fn global_fanticon_include_is_builtin_idempotent_and_usable() {
        let source = r#"
         INCLUDE FANTICON.INC
         INCLUDE /fanticon.inc
         ORG   $8000
         PMC   SET_BANK;BANK_VRAM;2
         PMC   ACK_IRQ;IRQ_VBLANK
         PMC   SET_IRQS;IRQ_RASTER
         PMC   SET_VIDEO;VIDEO_TILEMAP;VIDEO_ALL
         PMC   SET_BITMAP;2;VIDEO_ALL
         PMC   SET_BACKDROP;$49
         PMC   SET_SCROLL;$1234;$5678
         PMC   SET_RASTER;220;199
         PMC   SET_COLOR;3;RGB332_RED
         PMC   UPLOAD_TILE;1;DATA
         PMC   FILL_TILEMAP;1;2
         PMC   SET_SPRITE;0;$1F0;20;4;$C1
         PMC   SET_TONE;PULSE1_CONTROL;$CC;$1BD
         PMC   SET_NOISE;$88;13
         PMC   SET_AUDIO_MASTER;15
         PMC   SILENCE_AUDIO
         PMC   START_TIMER;TIMER0_RELOADL;1000
         PMC   STOP_TIMER;TIMER0_RELOADL
         PMC   READ_FRAME16;$20
         PMC   READ_TIMER16;TIMER0_RELOADL;$22
         PMC   WAIT_VBLANK
         PMC   WAIT_NEXT_VBLANK
         PMC   PUSH_BANK
         PMC   POP_BANK
         PMC   PUSH_AXY
         PMC   POP_YXA
         PMC   STORE16;$24;$CAFE
         PMC   COPY16;$26;$24
         PMC   ADD16;$24;$1234
         PMC   SUB16;$24;$1234
         PMC   INC16;$24
         PMC   DEC16;$24
         PMC   EMIT_VRAM_COPY;VCOPY;$30;$32;$34;$0200
         PMC   EMIT_PAD_SCROLL;SCROLL;$36;$38
DATA     DS    TILE_BYTES
"#;
        let program = assemble_with_loader("main.asm", source, |_| {
            panic!("the built-in include must not call the project loader")
        })
        .unwrap();
        assert_eq!(program.origin, 0x8000);
        assert_eq!(program.symbols["BANK_KIND"], 0xc000);
        assert_eq!(program.symbols["PAD_START"], 0x80);
        assert_eq!(program.symbols["VRAM_SPR_CPU"], 0xb000);
        assert!(program.symbols["VCOPY"] >= 0x8000);
        assert!(program.symbols["SCROLL"] > program.symbols["VCOPY"]);
    }

    #[test]
    fn standard_emitters_require_the_fixed_cartridge_section() {
        let wrong_bank = r#"
         INCLUDE FANTICON.INC
         BANK  3
         ORG   $8000
         PMC   EMIT_PAD_SCROLL;SCROLL;$20;$22
"#;
        let diagnostics =
            assemble_cartridge_with_loader("main.asm", wrong_bank, |_| Err("not found".to_owned()))
                .unwrap_err();
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.source == "main.asm"
                && diagnostic.line == 5
                && diagnostic.message == "REQUIRE_FIXED failed while BANK 3 is selected"
        }));

        let fixed = r#"
         INCLUDE FANTICON.INC
         FIXED
         ORG   $C100
         PMC   EMIT_PAD_SCROLL;SCROLL;$20;$22
RESET    JMP   RESET
NMI      RTI
IRQ      RTI
         ORG   VECTOR_NMI
         DA    NMI,RESET,IRQ
"#;
        let cartridge =
            assemble_cartridge_with_loader("main.asm", fixed, |_| Err("not found".to_owned()))
                .unwrap();
        assert_eq!(cartridge.symbols["SCROLL"].section, SymbolSection::Fixed);
    }

    #[test]
    fn undocumented_opcodes_cover_every_supported_addressing_mode() {
        use Mode::{
            Absolute as Abs, AbsoluteX as AbsX, AbsoluteY as AbsY, Immediate as Imm,
            Implied as Imp, IndirectX as IndX, IndirectY as IndY, ZeroPage as Zp, ZeroPageX as ZpX,
            ZeroPageY as ZpY,
        };

        let cases = [
            ("KIL", Imp, 0x02),
            ("JAM", Imp, 0x02),
            ("SLO", IndX, 0x03),
            ("SLO", Zp, 0x07),
            ("SLO", Abs, 0x0f),
            ("SLO", IndY, 0x13),
            ("SLO", ZpX, 0x17),
            ("SLO", AbsY, 0x1b),
            ("SLO", AbsX, 0x1f),
            ("RLA", IndX, 0x23),
            ("RLA", Zp, 0x27),
            ("RLA", Abs, 0x2f),
            ("RLA", IndY, 0x33),
            ("RLA", ZpX, 0x37),
            ("RLA", AbsY, 0x3b),
            ("RLA", AbsX, 0x3f),
            ("SRE", IndX, 0x43),
            ("SRE", Zp, 0x47),
            ("SRE", Abs, 0x4f),
            ("SRE", IndY, 0x53),
            ("SRE", ZpX, 0x57),
            ("SRE", AbsY, 0x5b),
            ("SRE", AbsX, 0x5f),
            ("RRA", IndX, 0x63),
            ("RRA", Zp, 0x67),
            ("RRA", Abs, 0x6f),
            ("RRA", IndY, 0x73),
            ("RRA", ZpX, 0x77),
            ("RRA", AbsY, 0x7b),
            ("RRA", AbsX, 0x7f),
            ("SAX", IndX, 0x83),
            ("SAX", Zp, 0x87),
            ("SAX", Abs, 0x8f),
            ("SAX", ZpY, 0x97),
            ("LAX", Imm, 0xab),
            ("LAX", IndX, 0xa3),
            ("LAX", Zp, 0xa7),
            ("LAX", Abs, 0xaf),
            ("LAX", IndY, 0xb3),
            ("LAX", ZpY, 0xb7),
            ("LAX", AbsY, 0xbf),
            ("DCP", IndX, 0xc3),
            ("DCP", Zp, 0xc7),
            ("DCP", Abs, 0xcf),
            ("DCP", IndY, 0xd3),
            ("DCP", ZpX, 0xd7),
            ("DCP", AbsY, 0xdb),
            ("DCP", AbsX, 0xdf),
            ("ISC", IndX, 0xe3),
            ("ISC", Zp, 0xe7),
            ("ISC", Abs, 0xef),
            ("ISC", IndY, 0xf3),
            ("ISC", ZpX, 0xf7),
            ("ISC", AbsY, 0xfb),
            ("ISC", AbsX, 0xff),
            ("ISB", Zp, 0xe7),
            ("ANC", Imm, 0x0b),
            ("ALR", Imm, 0x4b),
            ("ARR", Imm, 0x6b),
            ("XAA", Imm, 0x8b),
            ("AXS", Imm, 0xcb),
            ("SBX", Imm, 0xcb),
            ("AHX", IndY, 0x93),
            ("AHX", AbsY, 0x9f),
            ("SHY", AbsX, 0x9c),
            ("SHX", AbsY, 0x9e),
            ("TAS", AbsY, 0x9b),
            ("LAS", AbsY, 0xbb),
            ("NOP", Imp, 0xea),
            ("NOP", Imm, 0x80),
            ("NOP", Zp, 0x04),
            ("NOP", ZpX, 0x14),
            ("NOP", Abs, 0x0c),
            ("NOP", AbsX, 0x1c),
        ];

        for (mnemonic, mode, expected) in cases {
            assert!(is_mnemonic(mnemonic), "{mnemonic} is missing from the parser vocabulary");
            assert_eq!(opcode(mnemonic, mode), Some(expected), "{mnemonic} {mode:?}");
        }
    }

    #[test]
    fn assembles_undocumented_mnemonics_aliases_and_canonical_nops() {
        let source = r#"
         NOP
         NOP   #$12
         NOP   $12
         NOP   $12,X
         NOP   $1234
         NOP   $1234,X
         KIL
         JAM
         ANC   #$12
         ISB   $12
         SBX   #$12
         LAX   #$12
         AHX   ($12),Y
"#;
        let program = assemble(source).unwrap();
        assert_eq!(
            program.bytes,
            [
                0xea, 0x80, 0x12, 0x04, 0x12, 0x14, 0x12, 0x0c, 0x34, 0x12, 0x1c, 0x34, 0x12, 0x02,
                0x02, 0x0b, 0x12, 0xe7, 0x12, 0xcb, 0x12, 0xab, 0x12, 0x93, 0x12,
            ]
        );
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

    #[test]
    fn cartridge_sections_pack_banks_and_resolve_bank_aware_symbols() {
        let source = r#"
         BANK  2
         ORG   $8000
LEVEL    DFB   $42
         FIXED
         ORG   $C100
RESET    LDA   #BANKOF(LEVEL)
         LDX   LEVEL
LOOP     JMP   LOOP
NMI      RTI
IRQ      RTI
         ORG   $FFFA
         DA    NMI,RESET,IRQ
"#;
        let program =
            assemble_cartridge_with_loader("cart.asm", source, |_| unreachable!()).unwrap();
        assert_eq!(program.rom_banks.len(), 3);
        assert_eq!(program.rom_banks[0], [0xff; 0x4000]);
        assert_eq!(program.rom_banks[2][0], 0x42);
        assert_eq!(&program.fixed_rom[0x100..0x105], &[0xa9, 2, 0xae, 0x00, 0x80]);
        assert_eq!(program.symbols["LEVEL"].section, SymbolSection::Bank(2));
        assert_eq!(program.symbols["RESET"].address, 0xc100);
        assert_eq!(program.symbols["NMI"].address, 0xc108);
        assert_eq!(&program.fixed_rom[0x3ffa..], &[0x08, 0xc1, 0x00, 0xc1, 0x09, 0xc1]);

        assert_eq!(program.bank_usage.len(), 4, "FIXED plus banks 0 through 2");
        assert_eq!(
            program.bank_usage[0],
            BankUsage { section: SymbolSection::Fixed, used: 16, capacity: 0x3f00 }
        );
        assert_eq!(
            program.bank_usage[1],
            BankUsage { section: SymbolSection::Bank(0), used: 0, capacity: 0x4000 }
        );
        assert_eq!(
            program.bank_usage[3],
            BankUsage { section: SymbolSection::Bank(2), used: 1, capacity: 0x4000 }
        );
        assert_eq!(program.bank_usage[3].free(), 0x4000 - 1);
    }

    #[test]
    fn cartridge_sections_reject_hidden_io_overlap_and_missing_vectors() {
        let source = " FIXED\n ORG $C000\n DFB 1";
        let diagnostics =
            assemble_cartridge_with_loader("bad.asm", source, |_| unreachable!()).unwrap_err();
        assert!(diagnostics.iter().any(|error| error.message.contains("hidden")));
        assert!(diagnostics.iter().any(|error| error.message.contains("vectors")));
    }
}
