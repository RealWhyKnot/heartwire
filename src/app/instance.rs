use std::fs::File;
use std::path::Path;

pub struct Instance {
    _lock: File,
}

pub fn acquire(dir: &Path) -> Option<Instance> {
    let _ = std::fs::create_dir_all(dir);
    let lock = File::create(dir.join("heartwire.lock")).ok()?;
    lock.try_lock().ok()?;
    Some(Instance { _lock: lock })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_instance_is_refused_until_the_first_exits() {
        let dir = std::env::temp_dir().join(format!("heartwire-instance-{}", std::process::id()));
        let first = acquire(&dir).expect("first instance gets the lock");
        assert!(acquire(&dir).is_none(), "second instance is refused");
        drop(first);
        assert!(
            acquire(&dir).is_some(),
            "the lock frees when the first exits"
        );
        let _ = std::fs::remove_file(dir.join("heartwire.lock"));
        let _ = std::fs::remove_dir(&dir);
    }
}
