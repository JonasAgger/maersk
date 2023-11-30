use std::collections::HashMap;

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub(crate) struct Image {
    config: ImageConfig,
}

impl Image {
    pub(crate) fn pull(repo: &str, tag: &str, dest: &str) -> Result<Image> {
        
        // Move from slice to string because we might override it.
        let mut repo = repo.to_string();
        if !repo.contains('/') {
            repo = format!("library/{}", repo);
        }

        println!("pulling image {}:{} to dir: {}", repo, tag, dest);

        let client = reqwest::blocking::Client::new();

        let auth_url = format!("https://auth.docker.io/token?service=registry.docker.io&scope=repository:{}:pull", repo);
        let token: Token  = client.get(auth_url).send()?.json()?;
        
        let manifest_url = format!("https://index.docker.io/v2/{}/manifests/{}", repo, tag);

        println!("Pulling manifest");
        let mut manifest_data = client
            .get(manifest_url)
            .bearer_auth(&token.token)
            .header(reqwest::header::ACCEPT, "application/vnd.docker.distribution.manifest.v2+json")
            .header(reqwest::header::ACCEPT, "application/vnd.docker.distribution.manifest.list.v2+json")
            .send()?;

        let content_type = manifest_data.headers().get(reqwest::header::CONTENT_TYPE).unwrap();

        if content_type.eq(&"application/vnd.docker.distribution.manifest.list.v2+json") {
            let image_manifest_list: ImageManifestList = manifest_data.json()?;

            // lets try to find the best version
            let manifest = image_manifest_list
                .manifests
                .iter()
                .filter(|mf| mf.platform.architecture.eq("amd64") && mf.platform.os.eq("linux"))
                .next()
                .expect("expected to find manifest");

            let manifest_url = format!("https://index.docker.io/v2/{}/manifests/{}", repo, manifest.digest);

            println!("Pulling single manifest because first was list");
            manifest_data = client
                .get(manifest_url)
                .bearer_auth(&token.token)
                .header(reqwest::header::ACCEPT, "application/vnd.docker.distribution.manifest.v2+json")
                .send()?;
        }
        else if content_type.eq(&"application/vnd.oci.image.index.v1+json") {
            return Self::handle_oci(repo, dest, client, token, manifest_data.json()?);
        }

        let manifest: ImageManifest = manifest_data.json()?;

        println!("Pulling Image Config");
        let image_config: ImageConfig = client
            .get(format!("https://index.docker.io/v2/{}/blobs/{}", repo, manifest.config.digest))
            .bearer_auth(&token.token)
            .header(reqwest::header::ACCEPT, "application/vnd.docker.container.image.v1+json")
            .send()?.json()?;

        for layer in manifest.layers {
            println!("Pulling Layer: {}", layer.digest);

            let blob_url = format!("https://index.docker.io/v2/{}/blobs/{}", repo, layer.digest);

            let blob_data = client
                .get(blob_url)
                .bearer_auth(&token.token)
                .header(reqwest::header::ACCEPT, "application/vnd.docker.distribution.manifest.v2+json")
                .send()?;

            use tar::Archive;
            let gz_reader = flate2::read::GzDecoder::new(blob_data);
            let mut tar_archive = Archive::new(gz_reader);
            tar_archive.unpack(dest).expect("failed to unpack");
        }

        Ok(Image {
            config: image_config,
        })
    } 



    fn handle_oci(repo: String, dest: &str, client: reqwest::blocking::Client, token: Token, image_manifest_list: ImageManifestList) -> Result<Image> {
        // lets try to find the best version
        let manifest = image_manifest_list
            .manifests
            .iter()
            .filter(|mf| mf.platform.architecture.eq("amd64") && mf.platform.os.eq("linux"))
            .next()
            .expect("expected to find manifest");

        let manifest_url = format!("https://index.docker.io/v2/{}/manifests/{}", repo, manifest.digest);

        println!("Pulling single manifest because first was list");
        let manifest_data = client
            .get(manifest_url)
            .bearer_auth(&token.token)
            .header(reqwest::header::ACCEPT, "application/vnd.oci.image.manifest.v1+json")
            .send()?;

        let manifest: ImageManifest = manifest_data.json()?;

        println!("Pulling Image Config");
        let image_config_data = client
            .get(format!("https://index.docker.io/v2/{}/blobs/{}", repo, manifest.config.digest))
            .bearer_auth(&token.token)
            .header(reqwest::header::ACCEPT, "application/vnd.oci.image.config.v1+json")
            .send()?;


        let image_config: ImageConfig = image_config_data.json()?;

        for layer in manifest.layers {
            println!("Pulling Layer: {}", layer.digest);

            let blob_url = format!("https://index.docker.io/v2/{}/blobs/{}", repo, layer.digest);

            let blob_data = client
                .get(blob_url)
                .bearer_auth(&token.token)
                .header(reqwest::header::ACCEPT, "application/vnd.oci.image.layer.v1.tar+gzip")
                .send()?;

            use tar::Archive;
            let gz_reader = flate2::read::GzDecoder::new(blob_data);
            let mut tar_archive = Archive::new(gz_reader);
            tar_archive.unpack(dest).expect("failed to unpack");
        }

        Ok(Image {
            config: image_config,
        })
    }


    pub(crate) fn config(&self) -> &ImageConfig {
        &self.config
    }
}

#[derive(Deserialize)]
struct Token {
    token: String
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageManifestList {
    pub manifests: Vec<Manifest>,
    pub media_type: String,
    pub schema_version: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub digest: String,
    pub media_type: String,
    pub platform: Platform,
    pub size: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Platform {
    pub architecture: String,
    pub os: String,
    pub variant: Option<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageManifest {
    pub schema_version: i64,
    pub media_type: String,
    pub config: Config,
    pub layers: Vec<Layer>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub media_type: String,
    pub size: i64,
    pub digest: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layer {
    pub media_type: String,
    pub size: i64,
    pub digest: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageConfig {
    pub architecture: String,
    pub config: ImageHostConfig,
    pub container: String,
    pub created: String,
    #[serde(rename = "docker_version")]
    pub docker_version: String,
    pub os: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageHostConfig {
    #[serde(rename = "Hostname")]
    pub hostname: String,
    #[serde(rename = "Domainname")]
    pub domainname: String,
    #[serde(rename = "User")]
    pub user: String,
    #[serde(rename = "AttachStdin")]
    pub attach_stdin: bool,
    #[serde(rename = "AttachStdout")]
    pub attach_stdout: bool,
    #[serde(rename = "AttachStderr")]
    pub attach_stderr: bool,
    #[serde(rename = "ExposedPorts")]
    pub exposed_ports: Option<HashMap<String, serde_json::Value>>,
    #[serde(rename = "Tty")]
    pub tty: bool,
    #[serde(rename = "OpenStdin")]
    pub open_stdin: bool,
    #[serde(rename = "StdinOnce")]
    pub stdin_once: bool,
    #[serde(rename = "Env")]
    pub env: Vec<String>,
    #[serde(rename = "Cmd")]
    pub cmd: Vec<String>,
    #[serde(rename = "Image")]
    pub image: String,
    #[serde(rename = "Volumes")]
    pub volumes: Option<HashMap<String, String>>,
    #[serde(rename = "WorkingDir")]
    pub working_dir: String,
    #[serde(rename = "Entrypoint")]
    pub entrypoint: Option<Vec<String>>,
}