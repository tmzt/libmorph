//! The persistent pipeline cache (docs/design/20261016_02_PIPELINE_CACHE_PLAN.md):
//! on Android with Vulkan, one `wgpu::PipelineCache` per run, loaded from
//! `<data_dir>/libmorph/<key>.bin` before `init` and saved back after
//! `init`, on suspend and on exit.

use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::io;
use std::path::{Path, PathBuf};

/// libmorph's own directory under the app's data directory.
const DIR: &str = "libmorph";

/// The cache and where its data is kept.
#[derive(Debug)]
pub(crate) struct PipelineCacheStore {
    cache: wgpu::PipelineCache,
    path: PathBuf,
    /// Hash of the last data read or written, to skip unchanged saves.
    saved: Option<u64>,
}

/// `<data_dir>/libmorph/<key>.bin`.
pub(crate) fn path_for(data_dir: &Path, key: &str) -> PathBuf {
    data_dir.join(DIR).join(format!("{key}.bin"))
}

/// The file's data: `None` when it is missing or unreadable.
pub(crate) fn read(path: &Path) -> Option<Vec<u8>> {
    match std::fs::read(path) {
        Ok(data) => Some(data),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            log::debug!("libmorph: pipeline cache: no file at {}", path.display());
            None
        }
        Err(error) => {
            log::warn!(
                "libmorph: pipeline cache: cannot read {}: {error}",
                path.display()
            );
            None
        }
    }
}

/// Write `data` whole: to `<path>.tmp`, then renamed over `path`.
pub(crate) fn write(path: &Path, data: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path)
}

fn hash(data: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    hasher.finish()
}

/// Whether data hashing to `hash` needs writing, after `saved`.
pub(crate) fn changed(saved: Option<u64>, data: &[u8]) -> Option<u64> {
    let h = hash(data);
    (saved != Some(h)).then_some(h)
}

impl PipelineCacheStore {
    /// The cache for this run, if the device was granted `PIPELINE_CACHE`
    /// and the adapter has a cache key (Vulkan). Only Android's runner,
    /// which has a data directory, opens one.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub(crate) fn open(
        device: &wgpu::Device,
        info: &wgpu::AdapterInfo,
        data_dir: &Path,
    ) -> Option<Self> {
        if !device.features().contains(wgpu::Features::PIPELINE_CACHE) {
            return None;
        }
        let key = wgpu::util::pipeline_cache_key(info)?;
        let path = path_for(data_dir, &key);
        let data = read(&path);
        // SAFETY: the data comes only from an earlier `get_data`: libmorph is
        // the file's only writer, and writes it whole through a rename
        // (`write`). wgpu checks its header and, with `fallback`, gives an
        // empty cache for data from another wgpu version, adapter or driver.
        // A file damaged after its header is not detected; wgpu's docs
        // accept that as the risk of keeping the data on disk.
        let cache = unsafe {
            device.create_pipeline_cache(&wgpu::PipelineCacheDescriptor {
                label: Some("libmorph"),
                data: data.as_deref(),
                fallback: true,
            })
        };
        match &data {
            Some(data) => log::info!("libmorph: pipeline cache: loaded {} bytes", data.len()),
            None => log::info!("libmorph: pipeline cache: new"),
        }
        Some(Self {
            cache,
            saved: data.as_deref().map(hash),
            path,
        })
    }

    pub(crate) fn cache(&self) -> &wgpu::PipelineCache {
        &self.cache
    }

    /// Write the cache's data if it changed since it was last read or
    /// written. A failure is logged; the run goes on.
    pub(crate) fn save(&mut self) {
        let Some(data) = self.cache.get_data() else {
            return;
        };
        let Some(h) = changed(self.saved, &data) else {
            return;
        };
        match write(&self.path, &data) {
            Ok(()) => {
                self.saved = Some(h);
                log::debug!("libmorph: pipeline cache: saved {} bytes", data.len());
            }
            Err(error) => log::warn!(
                "libmorph: pipeline cache: cannot write {}: {error}",
                self.path.display()
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "libmorph-pipeline-cache-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn the_path_is_libmorph_s_own_directory_and_the_key() {
        assert_eq!(
            path_for(Path::new("/data/files"), "wgpu_pipeline_cache_vulkan_1_2"),
            Path::new("/data/files/libmorph/wgpu_pipeline_cache_vulkan_1_2.bin")
        );
    }

    #[test]
    fn a_missing_file_is_no_data() {
        let dir = temp_dir("missing");
        assert_eq!(read(&path_for(&dir, "key")), None);
    }

    #[test]
    fn a_save_goes_through_a_temporary_file_and_leaves_none_behind() {
        let dir = temp_dir("save");
        let path = path_for(&dir, "key");
        write(&path, b"first").unwrap();
        write(&path, b"second").unwrap();
        assert_eq!(read(&path).as_deref(), Some(&b"second"[..]));
        let names: Vec<_> = std::fs::read_dir(dir.join(DIR))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["key.bin"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_unchanged_save_is_skipped() {
        let first = changed(None, b"data").expect("new data is written");
        assert_eq!(changed(Some(first), b"data"), None);
        assert!(changed(Some(first), b"other").is_some());
    }
}
