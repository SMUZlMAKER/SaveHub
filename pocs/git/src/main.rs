use std::{error::Error, fs, path::Path};

use git2::{IndexAddOption, Oid, Repository, RepositoryInitOptions, Signature};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn print_status(repo: &Repository) -> Result<()> {
    let mut options = git2::StatusOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_ignored(false);

    let changes = repo.statuses(Some(&mut options))?;
    println!("Изменения (git status):");

    for entry in changes.iter() {
        println!("  {:?}: {}", entry.status(), entry.path()?);
    }

    if changes.is_empty() {
        println!("  Рабочая копия чистая");
    }

    Ok(())
}

fn commit(repo: &Repository, message: &str) -> Result<Oid> {
    let mut index = repo.index()?;
    index.add_all(["*"], IndexAddOption::DEFAULT, None)?;
    index.write()?;

    let tree = repo.find_tree(index.write_tree()?)?;
    let signature = Signature::now("Git PoC", "poc@example.invalid")?;

    let parent = match repo.head() {
        Ok(head) => Some(head.peel_to_commit()?),
        Err(error) if error.code() == git2::ErrorCode::UnbornBranch => None,
        Err(error) => return Err(error.into()),
    };
    let parents: Vec<_> = parent.iter().collect();

    Ok(repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        message,
        &tree,
        &parents,
    )?)
}

fn checkout(repo: &Repository, reference: &str) -> Result<()> {
    let target = repo.revparse_single(reference)?;
    repo.checkout_tree(&target, Some(git2::build::CheckoutBuilder::new().safe()))?;
    repo.set_head(reference)?;

    Ok(())
}

fn demo(root: &Path) -> Result<()> {
    let mut options = RepositoryInitOptions::new();
    options.initial_head("main");
    let repo = Repository::init_opts(root, &options)?;

    fs::write(root.join(".gitignore"), "*.tmp\n")?;
    fs::write(root.join("session.tmp"), "ignored")?;
    fs::write(root.join("example.txt"), "version=1\n")?;

    print_status(&repo)?;

    let initial = commit(&repo, "Initial commit")?;

    assert!(
        repo.index()?
            .get_path(Path::new("session.tmp"), 0)
            .is_none()
    );
    assert!(repo.status_should_ignore(Path::new("session.tmp"))?);

    println!("init + commit + .gitignore: OK");

    repo.branch("example-branch", &repo.find_commit(initial)?, false)?;
    checkout(&repo, "refs/heads/example-branch")?;

    fs::write(root.join("example.txt"), "version=2\n")?;

    print_status(&repo)?;

    commit(&repo, "Update example file")?;

    checkout(&repo, "refs/heads/main")?;
    assert_eq!(fs::read_to_string(root.join("example.txt"))?.trim_end(), "version=1");

    checkout(&repo, "refs/heads/example-branch")?;
    assert_eq!(fs::read_to_string(root.join("example.txt"))?.trim_end(), "version=2");

    checkout(&repo, "refs/heads/main")?;

    println!("branch + checkout: OK");

    print_status(&repo)?;

    Ok(())
}

fn main() -> Result<()> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("demo");
    fs::create_dir(&directory).map_err(|error| {
        format!(
            "Не удалось создать {}: {error}. Если папка уже существует, переместите или удалите её перед повторным запуском.",
            directory.display()
        )
    })?;

    println!("Файлы PoC: {}", directory.display());
    demo(&directory)?;

    println!("OK");

    Ok(())
}
