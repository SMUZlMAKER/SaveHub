fn slugify(value: &str) -> String {
    let mut slug = String::new();
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_matches('-').to_owned()
}

pub fn name_to_branch(name: &str) -> String {
    slugify(&transliterate(name))
}

fn transliterate(text: &str) -> String {
    let mut result = String::new();

    for character in text.chars() {
        let replacement = match character {
            'а' => "a",
            'б' => "b",
            'в' => "v",
            'г' => "g",
            'д' => "d",
            'е' => "e",
            'ё' => "yo",
            'ж' => "zh",
            'з' => "z",
            'и' => "i",
            'й' => "y",
            'к' => "k",
            'л' => "l",
            'м' => "m",
            'н' => "n",
            'о' => "o",
            'п' => "p",
            'р' => "r",
            'с' => "s",
            'т' => "t",
            'у' => "u",
            'ф' => "f",
            'х' => "kh",
            'ц' => "ts",
            'ч' => "ch",
            'ш' => "sh",
            'щ' => "shch",
            'ъ' | 'ь' => "",
            'ы' => "y",
            'э' => "e",
            'ю' => "yu",
            'я' => "ya",

            'А' => "A",
            'Б' => "B",
            'В' => "V",
            'Г' => "G",
            'Д' => "D",
            'Е' => "E",
            'Ё' => "Yo",
            'Ж' => "Zh",
            'З' => "Z",
            'И' => "I",
            'Й' => "Y",
            'К' => "K",
            'Л' => "L",
            'М' => "M",
            'Н' => "N",
            'О' => "O",
            'П' => "P",
            'Р' => "R",
            'С' => "S",
            'Т' => "T",
            'У' => "U",
            'Ф' => "F",
            'Х' => "Kh",
            'Ц' => "Ts",
            'Ч' => "Ch",
            'Ш' => "Sh",
            'Щ' => "Shch",
            'Ъ' | 'Ь' => "",
            'Ы' => "Y",
            'Э' => "E",
            'Ю' => "Yu",
            'Я' => "Ya",

            other => {
                result.push(other);
                continue;
            }
        };

        result.push_str(replacement);
    }

    result
}
