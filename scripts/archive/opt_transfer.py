import re

with open('src/engine/transfer.rs', 'r') as f:
    content = f.read()

old_code = """pub fn convert_date(date_str: &str, from_format: &str, to_format: &str) -> String {
    if from_format == to_format {
        return date_str.to_string();
    }

    // Parse the date components based on from_format
    let parts: Vec<&str> = if date_str.contains('/') {
        date_str.split('/').collect()
    } else if date_str.contains('-') {
        date_str.split('-').collect()
    } else {
        return date_str.to_string(); // can't parse, return as-is
    };

    if parts.len() < 3 {
        return date_str.to_string();
    }

    let (day, month, year) = match from_format {
        "DD/MM/YYYY" | "DD-MM-YYYY" => (parts[0], parts[1], parts[2]),
        "MM/DD/YYYY" | "MM-DD-YYYY" => (parts[1], parts[0], parts[2]),
        "YYYY-MM-DD" | "YYYY/MM/DD" => (parts[2], parts[1], parts[0]),
        _ => return date_str.to_string(),
    };

    let sep = if to_format.contains('/') { "/" } else { "-" };

    match to_format {
        "DD/MM/YYYY" | "DD-MM-YYYY" => format!("{day}{sep}{month}{sep}{year}"),
        "MM/DD/YYYY" | "MM-DD-YYYY" => format!("{month}{sep}{day}{sep}{year}"),
        "YYYY-MM-DD" | "YYYY/MM/DD" => format!("{year}{sep}{month}{sep}{day}"),
        _ => date_str.to_string(),
    }
}"""

new_code = """pub fn convert_date(date_str: &str, from_format: &str, to_format: &str) -> String {
    if from_format == to_format {
        return date_str.to_string();
    }

    let sep_char = if date_str.contains('/') {
        '/'
    } else if date_str.contains('-') {
        '-'
    } else if date_str.contains('.') {
        '.'
    } else {
        return date_str.to_string();
    };

    let mut parts = date_str.split(sep_char);
    let p1 = parts.next().unwrap_or("");
    let p2 = parts.next().unwrap_or("");
    let p3 = parts.next().unwrap_or("");

    if p1.is_empty() || p2.is_empty() || p3.is_empty() {
        return date_str.to_string();
    }

    let (day, month, year) = match from_format {
        "DD/MM/YYYY" | "DD-MM-YYYY" | "DD.MM.YYYY" => (p1, p2, p3),
        "MM/DD/YYYY" | "MM-DD-YYYY" | "MM.DD.YYYY" => (p2, p1, p3),
        "YYYY-MM-DD" | "YYYY/MM/DD" | "YYYY.MM.DD" => (p3, p2, p1),
        _ => return date_str.to_string(),
    };

    let sep = if to_format.contains('/') { "/" } else if to_format.contains('-') { "-" } else { "." };

    match to_format {
        "DD/MM/YYYY" | "DD-MM-YYYY" | "DD.MM.YYYY" => format!("{day}{sep}{month}{sep}{year}"),
        "MM/DD/YYYY" | "MM-DD-YYYY" | "MM.DD.YYYY" => format!("{month}{sep}{day}{sep}{year}"),
        "YYYY-MM-DD" | "YYYY/MM/DD" | "YYYY.MM.DD" => format!("{year}{sep}{month}{sep}{day}"),
        _ => date_str.to_string(),
    }
}"""

content = content.replace(old_code, new_code)

with open('src/engine/transfer.rs', 'w') as f:
    f.write(content)
