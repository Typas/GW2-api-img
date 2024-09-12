use std::collections::HashMap;

use anyhow::Context;
use serde_json as sj;
use tokio_stream::{self as ts, StreamExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let specializations_json = get_meta("specializations").await?;
    let skill_ids_json = get_meta("skills").await?;
    let trait_ids_json = get_meta("traits").await?;
    let specialization_ids = to_ids(specializations_json)?;
    let skill_ids = to_ids(skill_ids_json)?;
    let trait_ids = to_ids(trait_ids_json)?;
    let specialization_full = get_data(&specialization_ids, "specializations").await?;
    let skills_full = get_data(&skill_ids, "skills").await?;
    let traits_full = get_data(&trait_ids, "traits").await?;
    let buffs = get_buffs(&traits_full)?;
    let specializations = shrink_specializations(specialization_full)?;
    let skills = shrink_skills(skills_full)?;
    let traits = shrink_traits(traits_full)?;
    let buff_markdown = buffs_to_markdown(buffs)?;
    let skill_markdown = skills_to_markdown(skills)?;
    let trait_markdown = traits_to_markdown(traits, specializations)?;

    buff_markdown
        .into_iter()
        .chain(trait_markdown.into_iter())
        .chain(skill_markdown.into_iter())
        .for_each(|s| println!("{s}"));

    Ok(())
}

async fn get_meta(category: &str) -> anyhow::Result<sj::Value> {
    let url = format!("https://api.guildwars2.com/v2/{}", category);
    let result = reqwest::get(url).await?.json::<sj::Value>().await?;
    Ok(result)
}

async fn get_data(ids: &[u64], category: &str) -> anyhow::Result<sj::Value> {
    // need to split and merge for each 200 elements
    // due to the limit of traits
    let id_chunks = ids.chunks(200);
    let urls: Vec<String> = id_chunks
        .map(|x| {
            x.iter()
                .map(|x| x.to_string())
                .collect::<Vec<_>>()
                .join(",")
        })
        .map(|s| format!("https://api.guildwars2.com/v2/{}?ids={}", category, s))
        .collect();

    let mut stream = ts::iter(urls);
    let mut v = Vec::new();
    while let Some(url) = stream.next().await {
        let result = reqwest::get(url).await?.json::<sj::Value>().await?;
        v.push(result);
    }
    let result: Vec<sj::Value> = v
        .into_iter()
        .map(|jsv| {
            jsv.as_array()
                .unwrap()
                .into_iter()
                .map(|x| x.clone())
                .collect::<Vec<sj::Value>>()
        })
        .flatten()
        .collect();

    Ok(sj::Value::from(result))
}

fn to_ids(json: sj::Value) -> anyhow::Result<Vec<u64>> {
    json.as_array()
        .context("not an array")?
        .into_iter()
        .map(|x| x.as_u64())
        .collect::<Option<Vec<u64>>>()
        .context("fail to convert to ids")
}

fn get_buffs(json: &sj::Value) -> anyhow::Result<HashMap<String, String>> {
    let map: Vec<HashMap<&str, &sj::Value>> = json
        .as_array()
        .context("input is not array")?
        .iter()
        .map(|item| {
            item.as_object()
                .expect("an object")
                .iter()
                .map(|(k, v)| (k.as_str(), v))
                .collect()
        })
        .collect();

    let mut result = HashMap::new();

    let shrinked: Vec<_> = map
        .into_iter()
        .filter(|m| m.get("facts").is_some())
        .map(|m| m.get("facts").unwrap().as_array().unwrap())
        .flatten()
        .map(|v| v.as_object().unwrap())
        .filter(|x| x.get("type").is_some_and(|t| t.as_str().unwrap() == "Buff"))
        .collect();

    for buff in shrinked {
        let s = buff
            .get("status")
            .context("cannot find status of a buff")?
            .as_str()
            .context("cannot convert buff status to string")?;
        if result.get(s).is_none() {
            result.insert(
                s.to_owned(),
                buff.get("icon")
                    .context("cannot find status of a buff")?
                    .as_str()
                    .context("cannot convert buff status to string")?
                    .to_owned(),
            );
        }
    }

    Ok(result)
}

fn shrink_skills(json: sj::Value) -> anyhow::Result<sj::Value> {
    let result: Vec<sj::Value> = json
        .as_array()
        .context("is not an array")?
        .into_iter()
        .map(|v| {
            v.as_object()
                .expect("an object")
                .into_iter()
                .filter(|(k, _)| match k.as_str() {
                    "name" | "icon" | "type" | "professions" => true,
                    _ => false,
                })
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<sj::Map<String, sj::Value>>()
                .into()
        })
        .filter(|v: &sj::Value| {
            v.as_object()
                .expect("an object")
                .iter()
                .all(|(k, v)| match k.as_str() {
                    "professions" => v.as_array().map_or(false, |u| u.len() == 1),
                    _ => true,
                })
        })
        .filter(|v| v.as_object().unwrap().get("type").is_some())
        .filter(|v| v.as_object().unwrap().get("professions").is_some())
        .collect();
    Ok(sj::Value::from(result))
}

fn shrink_traits(json: sj::Value) -> anyhow::Result<sj::Value> {
    let result: Vec<sj::Value> = json
        .as_array()
        .context("is not an array")?
        .into_iter()
        .map(|v| {
            v.as_object()
                .unwrap()
                .into_iter()
                .filter(|(k, _)| match k.as_str() {
                    "name" | "icon" | "specialization" => true,
                    _ => false,
                })
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<sj::Map<String, sj::Value>>()
                .into()
        })
        .collect();
    Ok(sj::Value::from(result))
}

fn shrink_specializations(json: sj::Value) -> anyhow::Result<HashMap<i32, (String, String)>> {
    let mut result = HashMap::new();
    for spec in json.as_array().context("is not an array")?.into_iter() {
        let id = spec
            .get("id")
            .context("cannot find id")?
            .as_u64()
            .context("cannot cast to u64")? as i32;
        let specialization = spec
            .get("name")
            .context("cannot find spec")?
            .as_str()
            .context("cannot cast to str")?
            .to_owned();
        let profession = spec
            .get("profession")
            .context("cannot find spec")?
            .as_str()
            .context("cannot cast to str")?
            .to_owned();
        result.insert(id, (profession, specialization));
    }

    Ok(result)
}

fn buffs_to_markdown(buffs: HashMap<String, String>) -> anyhow::Result<Vec<String>> {
    let mut result = Vec::new();
    result.push(format!("## Buffs"));
    for buff in buffs {
        result.push(format!("[{}]: {}", buff.0, buff.1));
    }
    Ok(result)
}

fn skills_to_markdown(json: sj::Value) -> anyhow::Result<Vec<String>> {
    let mut skills: Vec<_> = json
        .as_array()
        .context("is not an array")?
        .into_iter()
        .map(|v| {
            v.as_object()
                .unwrap()
                .into_iter()
                .map(|(x, y)| match x.as_str() {
                    "professions" => (x.clone(), y.as_array().unwrap().first().unwrap().clone()),
                    _ => (x.clone(), y.clone()),
                })
                .collect::<HashMap<String, sj::Value>>()
        })
        .collect();
    skills.sort_by_key(|x| {
        (
            x.get("professions").unwrap().as_str().unwrap().to_owned(),
            x.get("type").unwrap().as_str().unwrap().to_owned(),
            x.get("name").unwrap().as_str().unwrap().to_owned(),
        )
    });

    let (mut last_prof, mut last_type) = ("".to_owned(), "".to_owned());
    let mut result = Vec::new();
    for skill in skills {
        let prof = skill
            .get("professions")
            .unwrap()
            .as_str()
            .unwrap()
            .to_owned();
        let typ = skill.get("type").unwrap().as_str().unwrap().to_owned();
        if prof != last_prof {
            last_prof = prof;
            last_type = typ;
            result.push(format!("## {}", &last_prof));
            result.push(format!("### {}", &last_type));
        } else if typ != last_type {
            last_type = typ;
            result.push(format!("### {}", &last_type));
        }

        result.push(format!(
            "[{}]: {}",
            skill.get("name").unwrap().as_str().unwrap(),
            skill.get("icon").unwrap().as_str().unwrap()
        ));
    }
    Ok(result)
}

fn traits_to_markdown(
    mut json: sj::Value,
    spec_map: HashMap<i32, (String, String)>,
) -> anyhow::Result<Vec<String>> {
    for t in json.as_array_mut().context("is not an array")?.iter_mut() {
        let s = t
            .get("specialization")
            .context("no specialization")?
            .as_u64()
            .context("cannot cast to u64")? as i32;
        let prof = spec_map.get(&s).context("cannot find spec")?.0.clone();
        let spec = spec_map.get(&s).context("cannot find spec")?.1.clone();
        t.as_object_mut()
            .context("not an object")?
            .insert("profession".to_string(), sj::Value::String(prof));
        t.as_object_mut()
            .context("not an object")?
            .insert("spec_str".to_string(), sj::Value::String(spec));
    }
    let mut traits: Vec<_> = json
        .as_array()
        .context("is not an array")?
        .into_iter()
        .map(|v| {
            v.as_object()
                .unwrap()
                .into_iter()
                .map(|(x, y)| match x.as_str() {
                    _ => (x.clone(), y.clone()),
                })
                .collect::<HashMap<String, sj::Value>>()
        })
        .collect();
    traits.sort_by_key(|x| {
        (
            x.get("profession").unwrap().as_str().unwrap().to_owned(),
            x.get("spec_str").unwrap().as_str().unwrap().to_owned(),
            x.get("name").unwrap().as_str().unwrap().to_owned(),
        )
    });

    let (mut last_prof, mut last_spec) = ("".to_owned(), "".to_owned());
    let mut result = Vec::new();
    for t in traits {
        let prof = t
            .get("profession")
            .context("cannot get prof")?
            .as_str()
            .context("cannot cast to str")?
            .to_owned();
        let spec = t
            .get("spec_str")
            .context("cannot get spec")?
            .as_str()
            .context("cannot cast to str")?
            .to_owned();
        if prof != last_prof {
            last_prof = prof;
            last_spec = spec;
            result.push(format!("## {}", &last_prof));
            result.push(format!("### {}", &last_spec));
        } else if spec != last_spec {
            last_spec = spec;
            result.push(format!("### {}", &last_spec));
        }

        result.push(format!(
            "[{}]: {}",
            t.get("name").unwrap().as_str().unwrap(),
            t.get("icon").unwrap().as_str().unwrap()
        ));
    }
    Ok(result)
}
