//! `GetRandomPassword`: characters drawn without bias from the sets the caller keeps.

use aws_model_secrets_manager::keys::{
    GetRandomPasswordRequest as Ask, GetRandomPasswordResponse as Answer,
};
use aws_wire::ApiError;
use aws_wire::handler::{reply, request};
use aws_wire::service::{Call, Reply};

use crate::core::{Core, invalid_request};

const DEFAULT_LENGTH: usize = 32;
const PUNCTUATION: &str = "!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~";

impl Core {
    pub(crate) fn get_random_password(&mut self, call: &Call) -> Reply {
        let v = request::<Ask>(call)?;
        let length = v.get(Ask::PASSWORD_LENGTH).map_or(DEFAULT_LENGTH, |n| {
            usize::try_from(n).unwrap_or(DEFAULT_LENGTH)
        });
        let off = |key| v.get(key).unwrap_or(false);
        let excluded = v.get(Ask::EXCLUDE_CHARACTERS).unwrap_or_default();
        let mut sets: Vec<Vec<char>> = Vec::new();
        let mut add = |keep: bool, chars: &str| {
            let set: Vec<char> = chars.chars().filter(|c| !excluded.contains(*c)).collect();
            if keep && !set.is_empty() {
                sets.push(set);
            }
        };
        add(!off(Ask::EXCLUDE_UPPERCASE), "ABCDEFGHIJKLMNOPQRSTUVWXYZ");
        add(!off(Ask::EXCLUDE_LOWERCASE), "abcdefghijklmnopqrstuvwxyz");
        add(!off(Ask::EXCLUDE_NUMBERS), "0123456789");
        add(!off(Ask::EXCLUDE_PUNCTUATION), PUNCTUATION);
        add(v.get(Ask::INCLUDE_SPACE).unwrap_or(false), " ");
        if sets.is_empty() {
            // (memory)
            return Err(invalid_request(
                "The password character set is empty: no characters are left to choose from.",
            ));
        }
        let require_each = v.get(Ask::REQUIRE_EACH_INCLUDED_TYPE).unwrap_or(true);
        let password =
            draw(&sets, length, require_each).ok_or_else(|| ApiError::internal("no randomness"))?;
        reply::<Answer>(|o| {
            o.put(Answer::RANDOM_PASSWORD, &password);
        })
    }
}

/// `length` characters from the union of `sets`; with `each`, at least one from every set (when there is room).
fn draw(sets: &[Vec<char>], length: usize, each: bool) -> Option<String> {
    let all: Vec<char> = sets.iter().flatten().copied().collect();
    let mut out: Vec<char> = Vec::with_capacity(length);
    if each && length >= sets.len() {
        for set in sets {
            out.push(*set.get(aws_seal::random_below(set.len())?)?);
        }
    }
    while out.len() < length {
        out.push(*all.get(aws_seal::random_below(all.len())?)?);
    }
    // Shuffle, so the guaranteed characters are not always first.
    for i in (1..out.len()).rev() {
        out.swap(i, aws_seal::random_below(i + 1)?);
    }
    Some(out.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::draw;

    #[test]
    fn each_kind_appears_when_asked_for() {
        let sets = vec![vec!['a', 'b'], vec!['1', '2'], vec!['#']];
        for _ in 0..200 {
            let p = draw(&sets, 3, true).unwrap_or_default();
            assert!(
                p.contains('#')
                    && p.chars().any(|c| c.is_ascii_digit())
                    && p.chars().any(|c| c.is_ascii_lowercase()),
                "{p}"
            );
            assert_eq!(p.chars().count(), 3);
        }
        assert_eq!(draw(&sets, 40, false).map(|p| p.chars().count()), Some(40));
    }
}
