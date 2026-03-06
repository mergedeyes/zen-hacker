use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::Editor;
use rustyline::{Context, Helper};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::thread;
use std::time::Duration;
use std::net::Ipv4Addr;

// ==========================================
// 1. CORE DATA STRUCTURES
// ==========================================

#[derive(Serialize, Deserialize, Clone)]
enum VfsNode {
    File { content: String, owner: String },
    Directory { children: Vec<String>, owner: String },
}

impl VfsNode {
    fn new_file(content: String, owner: &str) -> Self {
        VfsNode::File { content, owner: owner.to_string() }
    }
    fn new_dir(owner: &str) -> Self {
        VfsNode::Directory { children: Vec::new(), owner: owner.to_string() }
    }
    fn owner(&self) -> &str {
        match self {
            VfsNode::File { owner, .. } => owner,
            VfsNode::Directory { owner, .. } => owner,
        }
    }
    fn can_access(&self, current_user: &str) -> bool {
        current_user == "root" || self.owner() == current_user
    }
}

// THE MOTHERBOARD: All OS state lives here now!
struct ZenOS {
    cwd: String,
    current_user: String,
    vfs: HashMap<String, VfsNode>,
    history: Vec<String>,
    aliases: HashMap<String, String>,
}

// ==========================================
// 2. THE COMMAND REGISTRY PATTERN
// ==========================================

// We wrap the function pointer in a Struct to break the infinite type cycle!
#[derive(Clone, Copy)]
struct CommandFn(pub fn(Vec<&str>, &mut ZenOS, Option<String>, &HashMap<String, CommandFn>) -> (bool, Option<String>));

fn build_registry() -> HashMap<String, CommandFn> {
    let mut reg: HashMap<String, CommandFn> = HashMap::new();
    reg.insert("alias".to_string(), CommandFn(cmd_alias));
    reg.insert("cat".to_string(), CommandFn(cmd_cat));
    reg.insert("cd".to_string(), CommandFn(cmd_cd));
    reg.insert("clear".to_string(), CommandFn(cmd_clear));
    reg.insert("cp".to_string(), CommandFn(cmd_cp));
    reg.insert("echo".to_string(), CommandFn(cmd_echo));
    reg.insert("exit".to_string(), CommandFn(cmd_exit));
    reg.insert("grep".to_string(), CommandFn(cmd_grep));
    reg.insert("help".to_string(), CommandFn(cmd_help));
    reg.insert("history".to_string(), CommandFn(cmd_history));
    reg.insert("ls".to_string(), CommandFn(cmd_ls));
    reg.insert("mkdir".to_string(), CommandFn(cmd_mkdir));
    reg.insert("mv".to_string(), CommandFn(cmd_mv));
    reg.insert("ping".to_string(), CommandFn(cmd_ping));
    reg.insert("pwd".to_string(), CommandFn(cmd_pwd));
    reg.insert("rm".to_string(), CommandFn(cmd_rm));
    reg.insert("sudo".to_string(), CommandFn(cmd_sudo));
    reg.insert("touch".to_string(), CommandFn(cmd_touch));
    reg.insert("whoami".to_string(), CommandFn(cmd_whoami));
    reg
}

// ==========================================
// 3. COMMAND FUNCTIONS
// ==========================================

fn cmd_alias(args: Vec<&str>, os: &mut ZenOS, _stdin: Option<String>, _reg: &HashMap<String, CommandFn>) -> (bool, Option<String>) {
    if args.is_empty() {
        let mut out = Vec::new();
        for (k, v) in os.aliases.iter() { out.push(format!("alias {}='{}'", k, v)); }
        (true, Some(out.join("\n")))
    } else {
        let alias_def = args.join(" ");
        if let Some((name, value)) = alias_def.split_once('=') {
            os.aliases.insert(name.trim().to_string(), value.trim().to_string());
        } else {
            println!("alias: usage: alias name=value");
        }
        (true, None)
    }
}

fn cmd_cat(mut args: Vec<&str>, os: &mut ZenOS, stdin: Option<String>, _reg: &HashMap<String, CommandFn>) -> (bool, Option<String>) {
    if args.is_empty() {
        if let Some(pipe_in) = stdin { return (true, Some(pipe_in)); }
        println!("cat: missing operand");
        return (true, None);
    }
    
    let file_name = args.remove(0);
    let full_path = resolve_path(file_name, &os.cwd);

    if let Some(node) = os.vfs.get(&full_path) {
        if !node.can_access(&os.current_user) {
            println!("cat: {}: Permission denied", file_name);
        } else {
            match node {
                VfsNode::Directory { .. } => println!("cat: '{}': Is a directory", file_name),
                VfsNode::File { content, .. } => return (true, Some(content.clone())),
            }
        }
    } else {
        println!("cat: {}: No such file", file_name);
    }
    (true, None)
}

fn cmd_cd(mut args: Vec<&str>, os: &mut ZenOS, _stdin: Option<String>, _reg: &HashMap<String, CommandFn>) -> (bool, Option<String>) {
    if args.is_empty() {
        os.cwd = "/home/user".to_string();
    } else {
        let path = args.remove(0);
        let full_path = resolve_path(path, &os.cwd);
        if let Some(node) = os.vfs.get(&full_path) {
            if !node.can_access(&os.current_user) {
                println!("cd: {}: Permission denied", path);
            } else {
                match node {
                    VfsNode::Directory { .. } => os.cwd = full_path,
                    VfsNode::File { .. } => println!("cd: {}: Not a directory", path),
                }
            }
        } else {
            println!("cd: {}: No such file or directory", path);
        }
    }
    (true, None)
}

fn cmd_clear(_args: Vec<&str>, _os: &mut ZenOS, _stdin: Option<String>, _reg: &HashMap<String, CommandFn>) -> (bool, Option<String>) {
    print!("\x1B[2J\x1B[1;1H");
    std::io::stdout().flush().unwrap();
    (true, None)
}

fn cmd_cp(mut args: Vec<&str>, os: &mut ZenOS, _stdin: Option<String>, _reg: &HashMap<String, CommandFn>) -> (bool, Option<String>) {
    if args.len() < 2 {
        println!("cp: missing file operand");
        return (true, None);
    }
    let src = args.remove(0);
    let dst = args.remove(0);
    let src_path = resolve_path(src, &os.cwd);
    let mut dst_path = resolve_path(dst, &os.cwd);

    if let Some(src_node) = os.vfs.get(&src_path) {
        if !src_node.can_access(&os.current_user) {
            println!("cp: cannot read '{}': Permission denied", src);
        } else {
            let cloned_node = src_node.clone();
            if let Some(VfsNode::Directory { .. }) = os.vfs.get(&dst_path) {
                let file_name = src_path.rsplit_once('/').map(|(_, name)| name).unwrap_or(&src_path);
                dst_path = resolve_path(file_name, &dst_path);
            }
            if let Err(e) = vfs_insert(&mut os.vfs, &dst_path, cloned_node, &os.current_user) {
                println!("cp: cannot create regular file '{}': {}", dst_path, e);
            }
        }
    } else {
        println!("cp: cannot stat '{}': No such file or directory", src);
    }
    (true, None)
}

fn cmd_echo(args: Vec<&str>, os: &mut ZenOS, stdin: Option<String>, _reg: &HashMap<String, CommandFn>) -> (bool, Option<String>) {
    let mut message_words = Vec::new();
    let mut target_file: Option<String> = None;
    let mut append_mode = false;

    let mut iter = args.into_iter();
    while let Some(word) = iter.next() {
        if word == ">" || word == ">>" {
            if word == ">>" { append_mode = true; }
            target_file = iter.next().map(|s| s.to_string());
            break;
        } else {
            message_words.push(word);
        }
    }

    let mut final_message = message_words.join(" ");
    if let Some(pipe_in) = stdin {
        if final_message.is_empty() { final_message = pipe_in; } 
        else { final_message = format!("{} {}", final_message, pipe_in); }
    }

    if let Some(file_name) = target_file {
        let full_path = resolve_path(&file_name, &os.cwd);
        if let Some(node) = os.vfs.get_mut(&full_path) {
            if !node.can_access(&os.current_user) {
                println!("echo: {}: Permission denied", file_name);
            } else if let VfsNode::File { content, .. } = node {
                if append_mode { content.push_str("\n"); content.push_str(&final_message); } 
                else { *content = final_message; }
            } else {
                println!("echo: {}: Is a directory", file_name);
            }
        } else {
            let new_file = VfsNode::new_file(final_message, &os.current_user);
            if let Err(e) = vfs_insert(&mut os.vfs, &full_path, new_file, &os.current_user) {
                println!("echo: cannot create file '{}': {}", file_name, e);
            }
        }
        (true, None)
    } else {
        (true, Some(final_message))
    }
}

fn cmd_exit(_args: Vec<&str>, _os: &mut ZenOS, _stdin: Option<String>, _reg: &HashMap<String, CommandFn>) -> (bool, Option<String>) {
    (false, None)
}

fn cmd_grep(mut args: Vec<&str>, os: &mut ZenOS, stdin: Option<String>, _reg: &HashMap<String, CommandFn>) -> (bool, Option<String>) {
    if args.is_empty() {
        println!("usage: grep <pattern> [<file>]");
        return (true, None);
    }
    
    let pattern = args.remove(0);
    let mut target_content = stdin.clone();

    if target_content.is_none() && !args.is_empty() {
        let file_arg = args.remove(0);
        let full_path = resolve_path(file_arg, &os.cwd);
        if let Some(node) = os.vfs.get(&full_path) {
            if !node.can_access(&os.current_user) {
                println!("grep: {}: Permission denied", file_arg);
            } else if let VfsNode::File { content, .. } = node {
                target_content = Some(content.clone());
            } else {
                println!("grep: {}: Is a directory", file_arg);
            }
        } else {
            println!("grep: {}: No such file", file_arg);
        }
    }

    if let Some(content) = target_content {
        let mut matches = Vec::new();
        for line in content.lines() {
            if line.contains(pattern) { matches.push(line.to_string()); }
        }
        (true, Some(matches.join("\n")))
    } else {
        (true, None)
    }
}

fn cmd_help(_args: Vec<&str>, _os: &mut ZenOS, _stdin: Option<String>, reg: &HashMap<String, CommandFn>) -> (bool, Option<String>) {
    let mut cmds: Vec<String> = reg.keys().cloned().collect();
    cmds.sort();
    let help_text = format!("HELP-MENU:\n  {}", cmds.join(", "));
    (true, Some(help_text))
}

fn cmd_history(_args: Vec<&str>, os: &mut ZenOS, _stdin: Option<String>, _reg: &HashMap<String, CommandFn>) -> (bool, Option<String>) {
    let mut out = Vec::new();
    for (i, cmd) in os.history.iter().enumerate() { out.push(format!("  {}  {}", i + 1, cmd)); }
    (true, Some(out.join("\n")))
}

fn cmd_ls(mut args: Vec<&str>, os: &mut ZenOS, _stdin: Option<String>, _reg: &HashMap<String, CommandFn>) -> (bool, Option<String>) {
    let target_path = if !args.is_empty() { resolve_path(args.remove(0), &os.cwd) } else { os.cwd.clone() };
    if let Some(node) = os.vfs.get(&target_path) {
        if !node.can_access(&os.current_user) {
            println!("ls: cannot open directory '{}': Permission denied", target_path);
        } else {
            match node {
                VfsNode::Directory { children, .. } => return (true, Some(children.join(" "))),
                VfsNode::File { .. } => {
                    let file_name = target_path.rsplit_once('/').map(|(_, name)| name).unwrap_or(&target_path);
                    return (true, Some(file_name.to_string()));
                }
            }
        }
    } else {
        println!("ls: cannot access '{}': No such file or directory", target_path);
    }
    (true, None)
}

fn cmd_mkdir(mut args: Vec<&str>, os: &mut ZenOS, _stdin: Option<String>, _reg: &HashMap<String, CommandFn>) -> (bool, Option<String>) {
    if args.is_empty() {
        println!("mkdir: missing operand");
    } else {
        let folder_name = args.remove(0);
        let full_path = resolve_path(folder_name, &os.cwd);
        let new_folder = VfsNode::new_dir(&os.current_user);
        if let Err(e) = vfs_insert(&mut os.vfs, &full_path, new_folder, &os.current_user) {
            println!("mkdir: cannot create directory '{}': {}", full_path, e);
        }
    }
    (true, None)
}

fn cmd_mv(mut args: Vec<&str>, os: &mut ZenOS, _stdin: Option<String>, _reg: &HashMap<String, CommandFn>) -> (bool, Option<String>) {
    if args.len() < 2 {
        println!("mv: missing file operand");
        return (true, None);
    }
    let src = args.remove(0);
    let dst = args.remove(0);
    let src_path = resolve_path(src, &os.cwd);
    let mut dst_path = resolve_path(dst, &os.cwd);

    if let Some(VfsNode::Directory { .. }) = os.vfs.get(&dst_path) {
        let file_name = src_path.rsplit_once('/').map(|(_, name)| name).unwrap_or(&src_path);
        dst_path = resolve_path(file_name, &dst_path);
    }

    match vfs_remove(&mut os.vfs, &src_path, &os.current_user) {
        Ok(src_node) => {
            if let Err(e) = vfs_insert(&mut os.vfs, &dst_path, src_node.clone(), &os.current_user) {
                let _ = vfs_insert(&mut os.vfs, &src_path, src_node, "root");
                println!("mv: cannot move to '{}': {}", dst_path, e);
            }
        }
        Err(e) => println!("mv: cannot stat '{}': {}", src, e),
    }
    (true, None)
}

fn cmd_ping(mut args: Vec<&str>, _os: &mut ZenOS, _stdin: Option<String>, _reg: &HashMap<String, CommandFn>) -> (bool, Option<String>) {
    if args.is_empty() {
        println!("ping: usage error: Destination address required");
        return (true, None);
    } 
    
    let target = args.remove(0);

    // THE VALIDATOR: Is it a valid IP or localhost?
    if target.parse::<Ipv4Addr>().is_err() && target != "localhost" {
        println!("ping: {}: Name or service not known", target);
        return (true, None);
    }

    println!("PING {} ({}): 56 data bytes", target, target);
    for i in 1..=4 {
        println!("64 bytes from {}: icmp_seq={} ttl=64 time=24.{} ms", target, i, i * 2);
        thread::sleep(Duration::from_millis(800));
    }
    println!("--- {} ping statistics ---", target);
    println!("4 packets transmitted, 4 received, 0% packet loss");
    
    (true, None)
}

fn cmd_pwd(_args: Vec<&str>, os: &mut ZenOS, _stdin: Option<String>, _reg: &HashMap<String, CommandFn>) -> (bool, Option<String>) {
    (true, Some(os.cwd.clone()))
}

fn cmd_rm(mut args: Vec<&str>, os: &mut ZenOS, _stdin: Option<String>, _reg: &HashMap<String, CommandFn>) -> (bool, Option<String>) {
    if args.is_empty() {
        println!("rm: missing operand");
    } else {
        let file_name = args.remove(0);
        let full_path = resolve_path(file_name, &os.cwd);
        if let Some(VfsNode::Directory { .. }) = os.vfs.get(&full_path) {
            println!("rm: cannot remove '{}': Is a directory", file_name);
        } else {
            match vfs_remove(&mut os.vfs, &full_path, &os.current_user) {
                Ok(_) => {}
                Err(e) => println!("rm: cannot remove '{}': {}", file_name, e),
            }
        }
    }
    (true, None)
}

fn cmd_sudo(args: Vec<&str>, os: &mut ZenOS, stdin: Option<String>, reg: &HashMap<String, CommandFn>) -> (bool, Option<String>) {
    if args.is_empty() {
        println!("usage: sudo <command>");
        (true, None)
    } else {
        let old_user = os.current_user.clone();
        os.current_user = String::from("root");
        
        let full_cmd = args.join(" ");
        let result = execute_command(&full_cmd, os, reg, stdin);
        
        os.current_user = old_user;
        result
    }
}

fn cmd_touch(mut args: Vec<&str>, os: &mut ZenOS, _stdin: Option<String>, _reg: &HashMap<String, CommandFn>) -> (bool, Option<String>) {
    if args.is_empty() {
        println!("touch: missing file operand");
    } else {
        let file_name = args.remove(0);
        let full_path = resolve_path(file_name, &os.cwd);
        if !os.vfs.contains_key(&full_path) {
            let new_file = VfsNode::new_file(String::new(), &os.current_user);
            if let Err(e) = vfs_insert(&mut os.vfs, &full_path, new_file, &os.current_user) {
                println!("touch: cannot touch '{}': {}", file_name, e);
            }
        }
    }
    (true, None)
}

fn cmd_whoami(_args: Vec<&str>, os: &mut ZenOS, _stdin: Option<String>, _reg: &HashMap<String, CommandFn>) -> (bool, Option<String>) {
    (true, Some(os.current_user.clone()))
}

// ==========================================
// 4. THE EXECUTION DISPATCHER
// ==========================================

fn execute_command(
    input: &str,
    os: &mut ZenOS,
    registry: &HashMap<String, CommandFn>,
    stdin: Option<String>,
) -> (bool, Option<String>) {
    let mut parts = input.split_whitespace();
    
    let cmd_name = match parts.next() {
        Some(name) => name,
        None => return (true, None),
    };

    // THE ALIAS ENGINE!
    if os.aliases.contains_key(cmd_name) {
        let expanded = os.aliases.remove(cmd_name).unwrap();
        let remainder: Vec<&str> = parts.collect();
        let new_input = if remainder.is_empty() { expanded.clone() } else { format!("{} {}", expanded, remainder.join(" ")) };
        
        // Recurse with the expanded command
        let result = execute_command(&new_input, os, registry, stdin);
        
        // Put the alias back
        os.aliases.insert(cmd_name.to_string(), expanded);
        return result;
    }

    // DISPATCH TO REGISTRY
    if let Some(command_function) = registry.get(cmd_name) {
        let args: Vec<&str> = parts.collect();
        // Notice the `.0` here to unwrap the struct!
        return command_function.0(args, os, stdin, registry); 
    } else {
        println!("Command {} not found!", cmd_name);
        (true, None)
    }
}

// ==========================================
// 5. VFS HELPERS
// ==========================================

fn vfs_insert(vfs: &mut HashMap<String, VfsNode>, full_path: &str, node: VfsNode, current_user: &str) -> Result<(), &'static str> {
    if let Some((parent_path, file_name)) = full_path.rsplit_once('/') {
        let actual_parent = if parent_path == "" { "/" } else { parent_path };
        if let Some(parent_node) = vfs.get_mut(actual_parent) {
            if !parent_node.can_access(current_user) { return Err("Permission denied"); }
            if let VfsNode::Directory { children, .. } = parent_node {
                if !children.contains(&file_name.to_string()) { children.push(file_name.to_string()); }
            } else { return Err("Not a directory"); }
        } else { return Err("No such parent directory"); }
        vfs.insert(full_path.to_string(), node);
        return Ok(());
    }
    Err("Invalid path")
}

fn vfs_remove(vfs: &mut HashMap<String, VfsNode>, full_path: &str, current_user: &str) -> Result<VfsNode, &'static str> {
    let node = match vfs.get(full_path) {
        Some(n) => n,
        None => return Err("No such file or directory"),
    };
    if !node.can_access(current_user) { return Err("Permission denied"); }
    if let Some((parent_path, file_name)) = full_path.rsplit_once('/') {
        let actual_parent = if parent_path == "" { "/" } else { parent_path };
        if let Some(parent_node) = vfs.get_mut(actual_parent) {
            if !parent_node.can_access(current_user) { return Err("Permission denied on parent directory"); }
            if let VfsNode::Directory { children, .. } = parent_node {
                children.retain(|child| child != file_name);
            }
        }
    }
    Ok(vfs.remove(full_path).unwrap())
}

fn resolve_path(target: &str, cwd: &str) -> String {
    let raw_path = if target.starts_with("/") { target.to_string() } 
    else if target == "~" { "/home/user".to_string() } 
    else if cwd == "/" { format!("/{}", target) } 
    else { format!("{}/{}", cwd, target) };

    let mut segments = Vec::new();
    for segment in raw_path.split('/') {
        if segment == "" || segment == "." { continue; } 
        else if segment == ".." { segments.pop(); } 
        else { segments.push(segment); }
    }
    format!("/{}", segments.join("/"))
}

// ==========================================
// 6. RUSTYLINE AUTOCOMPLETE
// ==========================================

struct ZenHelper {
    commands: Vec<String>, // Upgraded to String so it can hold Aliases too!
    vfs_paths: Vec<String>,
    cwd: String,
}

impl Completer for ZenHelper {
    type Candidate = Pair;
    fn complete(&self, line: &str, pos: usize, _ctx: &Context<'_>) -> rustyline::Result<(usize, Vec<Pair>)> {
        let mut candidates = Vec::new();
        let word_start = line[..pos].rfind(' ').map(|i| i + 1).unwrap_or(0);
        let word_to_complete = &line[word_start..pos];

        for cmd in &self.commands {
            if cmd.starts_with(word_to_complete) {
                candidates.push(Pair { display: cmd.to_string(), replacement: cmd.to_string() });
            }
        }

        let is_absolute = word_to_complete.starts_with('/');
        let search_prefix = if is_absolute { word_to_complete.to_string() } 
        else if self.cwd == "/" { format!("/{}", word_to_complete) } 
        else { format!("{}/{}", self.cwd, word_to_complete) };

        for path in &self.vfs_paths {
            if path.starts_with(&search_prefix) && path != &self.cwd {
                let replacement = if is_absolute { path.clone() } else {
                    let prefix_len = if self.cwd == "/" { 1 } else { self.cwd.len() + 1 };
                    if path.len() > prefix_len { path[prefix_len..].to_string() } else { continue; }
                };

                let final_replacement = if let Some(slash_idx) = replacement[word_to_complete.len()..].find('/') {
                    replacement[..word_to_complete.len() + slash_idx + 1].to_string()
                } else { replacement };

                if !candidates.iter().any(|c| c.replacement == final_replacement) {
                    candidates.push(Pair { display: final_replacement.clone(), replacement: final_replacement });
                }
            }
        }
        Ok((word_start, candidates))
    }
}

impl Hinter for ZenHelper {
    type Hint = String;
    fn hint(&self, _line: &str, _pos: usize, _ctx: &Context<'_>) -> Option<String> { None }
}
impl Highlighter for ZenHelper {}
impl Validator for ZenHelper {}
impl Helper for ZenHelper {}

// ==========================================
// 7. BOOTLOADER & MAIN LOOP
// ==========================================

fn main() {
    let mut initial_vfs: HashMap<String, VfsNode> = if let Ok(saved_data) = fs::read_to_string("vfs_data.json") {
        match serde_json::from_str(&saved_data) {
            Ok(valid_vfs) => valid_vfs,
            Err(_) => {
                println!("[SYSTEM] Outdated VFS format detected. Initiating data migration...");
                let mut raw_json: HashMap<String, serde_json::Value> = serde_json::from_str(&saved_data).unwrap();
                for (path, node) in raw_json.iter_mut() {
                    let owner = if path.starts_with("/home/user") { "user" } else { "root" };
                    if let Some(obj) = node.get_mut("File").and_then(|v| v.as_object_mut()) {
                        if !obj.contains_key("owner") { obj.insert("owner".to_string(), serde_json::Value::String(owner.to_string())); }
                    } else if let Some(obj) = node.get_mut("Directory").and_then(|v| v.as_object_mut()) {
                        if !obj.contains_key("owner") { obj.insert("owner".to_string(), serde_json::Value::String(owner.to_string())); }
                    }
                }
                let upgraded_vfs: HashMap<String, VfsNode> = serde_json::from_value(serde_json::to_value(&raw_json).unwrap()).unwrap();
                let patched_data = serde_json::to_string_pretty(&upgraded_vfs).unwrap();
                fs::write("vfs_data.json", patched_data).unwrap();
                println!("[SYSTEM] Migration complete. Booting...");
                upgraded_vfs
            }
        }
    } else {
        let mut default_vfs = HashMap::new();
        default_vfs.insert("/".to_string(), VfsNode::new_dir("root"));
        default_vfs.insert("/home".to_string(), VfsNode::new_dir("root"));
        default_vfs.insert("/home/user".to_string(), VfsNode::new_dir("user"));
        default_vfs
    };

    let mut rl = Editor::new().unwrap();
    let mut history_list: Vec<String> = Vec::new();
    
    if let Ok(history_data) = std::fs::read_to_string(".zen_history") {
        for line in history_data.lines() {
            history_list.push(line.to_string());
            let _ = rl.add_history_entry(line);
        }
    }

    let mut default_aliases = HashMap::new();
    default_aliases.insert("please".to_string(), "sudo".to_string()); // Default easter egg alias!

    // Initialize our Motherboard State
    let mut os = ZenOS {
        cwd: String::from("/home/user"),
        current_user: String::from("user"),
        vfs: initial_vfs,
        history: history_list,
        aliases: default_aliases,
    };

    let registry = build_registry();

    let mut helper = ZenHelper {
        commands: registry.keys().cloned().collect(), // Automatically pulls from Registry!
        vfs_paths: Vec::new(),
        cwd: String::new(),
    };
    
    // Inject Aliases into autocomplete on boot
    helper.commands.extend(os.aliases.keys().cloned());
    rl.set_helper(Some(helper));

    // Bootloader .zenrc execution
    let rc_content = if let Some(VfsNode::File { content, .. }) = os.vfs.get("/home/user/.zenrc") { Some(content.clone()) } else { None };
    if let Some(rc_data) = rc_content {
        for line in rc_data.lines() {
            let clean_line = line.trim();
            if !clean_line.is_empty() {
                let pipe_segments: Vec<&str> = clean_line.split('|').collect();
                let mut current_pipe_data: Option<String> = None;
                for segment in pipe_segments {
                    let (_, output) = execute_command(segment.trim(), &mut os, &registry, current_pipe_data);
                    current_pipe_data = output;
                }
                if let Some(final_text) = current_pipe_data {
                    if !final_text.is_empty() { println!("{}", final_text); }
                }
            }
        }
    }
    
    // Main Shell Loop
    loop {
        if let Some(helper) = rl.helper_mut() {
            helper.vfs_paths = os.vfs.keys().cloned().collect();
            helper.cwd = os.cwd.clone();
            
            // Dynamic Autocomplete updating!
            let mut all_cmds: Vec<String> = registry.keys().cloned().collect();
            all_cmds.extend(os.aliases.keys().cloned());
            helper.commands = all_cmds;
        }

        let prompt = if os.cwd.starts_with("/home/user") {
            format!("{}@sim-os:{}$ ", os.current_user, os.cwd.replace("/home/user", "~"))
        } else {
            format!("{}@sim-os:{}$ ", os.current_user, os.cwd)
        };

        let line = match rl.readline(&prompt) {
            Ok(line) => {
                let _ = rl.add_history_entry(line.as_str());
                line
            }
            Err(ReadlineError::Interrupted) => continue,
            Err(ReadlineError::Eof) => break,
            Err(err) => { println!("Error: {:?}", err); break; }
        };

        let history_input = line.trim().to_string();
        if history_input.is_empty() { continue; }
        os.history.push(history_input.clone());

        // Pipeline execution
        let pipe_segments: Vec<&str> = history_input.split('|').collect();
        let mut current_pipe_data: Option<String> = None;
        let mut keep_running = true;

        for segment in pipe_segments {
            let (should_continue, output) = execute_command(segment.trim(), &mut os, &registry, current_pipe_data);
            keep_running = should_continue;
            current_pipe_data = output;
            if !keep_running { break; }
        }

        if let Some(final_text) = current_pipe_data {
            if !final_text.is_empty() { println!("{}", final_text); }
        }
        if !keep_running { break; }
    }

    let json_data = serde_json::to_string_pretty(&os.vfs).unwrap();
    fs::write("vfs_data.json", json_data).unwrap();
    fs::write(".zen_history", os.history.join("\n")).unwrap();
    println!("State and history saved. Stay zen!");
}