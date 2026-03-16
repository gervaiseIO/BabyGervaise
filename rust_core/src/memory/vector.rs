use anyhow::Result;

pub fn vectorize_text(text: &str, dimensions: usize) -> Vec<f32> {
    let mut vector = vec![0.0_f32; dimensions];
    if text.trim().is_empty() || dimensions == 0 {
        return vector;
    }

    let normalized = text.to_lowercase();
    let tokens: Vec<&str> = normalized.split_whitespace().collect();

    for token in &tokens {
        add_feature(&mut vector, token.as_bytes(), 1.0);
    }

    for pair in tokens.windows(2) {
        let phrase = format!("{}_{}", pair[0], pair[1]);
        add_feature(&mut vector, phrase.as_bytes(), 1.4);
    }

    let chars: Vec<char> = normalized.chars().collect();
    for window in chars.windows(3) {
        let trigram: String = window.iter().collect();
        add_feature(&mut vector, trigram.as_bytes(), 0.6);
    }

    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > 0.0 {
        for value in &mut vector {
            *value /= norm;
        }
    }

    vector
}

pub fn cosine_similarity(left: &[f32], right: &[f32]) -> f32 {
    if left.len() != right.len() || left.is_empty() {
        return 0.0;
    }
    left.iter()
        .zip(right.iter())
        .map(|(left_value, right_value)| left_value * right_value)
        .sum()
}

pub fn normalize_memory_text(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || character.is_whitespace() {
                character
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn vector_to_blob(vector: &[f32]) -> Vec<u8> {
    let mut blob = Vec::with_capacity(vector.len() * 4);
    for value in vector {
        blob.extend_from_slice(&value.to_le_bytes());
    }
    blob
}

pub fn blob_to_vector(blob: &[u8]) -> Result<Vec<f32>> {
    let mut vector = Vec::with_capacity(blob.len() / 4);
    for chunk in blob.chunks_exact(4) {
        vector.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    Ok(vector)
}

fn add_feature(vector: &mut [f32], feature: &[u8], weight: f32) {
    let hash = stable_hash(feature);
    let index = (hash as usize) % vector.len();
    let sign = if hash & 1 == 0 { 1.0 } else { -1.0 };
    vector[index] += sign * weight;
}

fn stable_hash(input: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in input {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
