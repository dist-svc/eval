
use std::collections::HashMap;
use std::iter::FromIterator;

use log::{trace, debug, info};

use bollard::{Docker, API_DEFAULT_VERSION};

use bollard::container::{
    CreateContainerOptions, StartContainerOptions, 
    StopContainerOptions, RemoveContainerOptions,

    Stats as BollardStats, 
    Config as CreateConfig};

use bollard::service::{HostConfig, PortBinding};
use bollard::models::{Mount, MountTypeEnum};

use rolling_stats::Stats;
use strum_macros::{EnumVariantNames, EnumString, Display};

use crate::error::Error;
use super::SocketKind;



#[derive(PartialEq, Clone, Debug, EnumString, Display, EnumVariantNames)]
#[strum(serialize_all = "kebab_case")]
pub enum DockerMode {
    Local,
    Http,
    None,
}


pub struct ContainerStats {
    cpu_stats: Stats<f64>,
    mem_stats: Stats<f64>,
}

impl ContainerStats {
    /// Create a new container-stats object
    pub fn new() -> Self {
        Self {
            cpu_stats: Stats::new(),
            mem_stats: Stats::new(),
        }
    }

    /// Update stats tracking, returns cpu and memory use
    pub fn update(&mut self, stats: &BollardStats) -> Option<(f64, f64)> {
        // From https://stackoverflow.com/questions/30271942/get-docker-container-cpu-usage-as-percentage

        trace!("stats: {:?}", stats);

        let mem = match stats.memory_stats.usage.map(|v| v as f64 / 1042f64 / 1024f64) {
            Some(m) => m,
            None => return None,
        };

        self.mem_stats.update(mem);


        let (system_cpu, last_system_cpu) = match (stats.cpu_stats.system_cpu_usage, stats.precpu_stats.system_cpu_usage) {
            (Some(s), Some(l)) => (s, l),
            _ => return None,
        };

        let (app_cpu, last_app_cpu) = (stats.cpu_stats.cpu_usage.total_usage, stats.precpu_stats.cpu_usage.total_usage);

        let num_cpus = stats.cpu_stats.online_cpus.unwrap();

        
        // calculate the change for the cpu usage of the container in between readings
        let cpu_delta = app_cpu as f64 - last_app_cpu as f64;

        // calculate the change for the entire system between readings
        let system_delta = system_cpu as f64 - last_system_cpu as f64;

        // Calculate the percentage used by the container
        let cpu_percent = (cpu_delta / system_delta) * num_cpus as f64 * 100.0;

        self.cpu_stats.update(cpu_percent);

        Some((cpu_percent, mem))
    }

    /// Fetch current runtime statistics
    pub fn stats(&self) -> (Stats<f64>, Stats<f64>) {
        (self.cpu_stats.clone(), self.mem_stats.clone())
    }
}

pub async fn setup(mode: &DockerMode, target: &str) -> Result<Option<Docker>, Error> {
    // Connect to docker target if enabled
    match mode {
        DockerMode::Http => {
            let docker_target = format!("{}:{}", target, 2376);
            debug!("Connecting to docker host: {}", docker_target);

            let d = Docker::connect_with_http(&docker_target, 30, API_DEFAULT_VERSION)?;

            Ok(Some(d))
        },
        DockerMode::Local => {
            let d = Docker::connect_with_local_defaults()?;

            Ok(Some(d))
        }
        DockerMode::None => Ok(None),
    }
}

pub async fn container_start(remote: &mut Docker, index: u32, image: &str, command: Option<&str>, ports: &[(u16, SocketKind)], env: &[String]) -> Result<(String, String), Error> {
    info!("Setting up container for image: {}", image);

    let name = format!("test-{}", index);

    let create_options = Some(CreateContainerOptions{
        name: &name,
    });

    let expose_ports: Vec<_> = ports.iter().map(|v| {
        (format!("{}/{}", v.0, v.1), HashMap::<(), ()>::new()) 
    }).collect();
    let port_bindings: Vec<_> = ports.iter().map(|v| {
        (format!("{}/{}", v.0, v.1), Some(vec![PortBinding{host_ip: None, host_port: Some(format!("{}", v.0))}]))
    }).collect();

    // TMPFS mount for /run
    let mounts = vec![
        Mount{ target: Some("/run".to_string()), typ: Some(MountTypeEnum::TMPFS), ..Default::default() },
        Mount{ target: Some("/root/.certs".to_string()), source: Some("/etc/certs/iot-perf".to_string()), typ: Some(MountTypeEnum::BIND), ..Default::default() },
    ];

    let config = CreateConfig {
        image: Some(image.to_string()),
        exposed_ports: Some(HashMap::from_iter(expose_ports)),
        cmd: command.map(|v| v.split(" ").map(|c| c.to_string() ).collect() ),
        env: Some(env.to_vec()),
        host_config: Some(HostConfig {
            port_bindings: Some(HashMap::from_iter(port_bindings)),
            mounts: Some(mounts),
            // seccomp:unconfined due to https://github.com/moby/moby/issues/40734
            security_opt: Some(vec!["seccomp:unconfined".to_string()]),
            ..Default::default()
        }),
        ..Default::default()
    };

    info!("Attempting to remove existing container");

    // First, remove "test" container if exists
    let _ = remote.remove_container(&name, Some(RemoveContainerOptions{ force: true, ..Default::default() })).await;

    info!("Creating container {}", name);
    
    // Then create the new "test" container
    let info = remote.create_container(create_options, config)
                .await?;

    debug!("Starting container {}", name);

    let _s = remote.start_container(&name, None::<StartContainerOptions<String>>)
                .await?;

    info!("Container {} started!", name);

    Ok((info.id, name))
}

pub async fn container_stop(remote: &mut Docker, name: &str) -> Result<(), Error> {

    info!("Stopping container {}", name);
    let options = Some(StopContainerOptions{
        t: 10,
    });
    remote.stop_container(name, options).await?;

    debug!("Removing container {}", name);
    let options = Some(RemoveContainerOptions{
        force: true,
        ..Default::default()
    });
    remote.remove_container(name, options).await?;

    info!("Container {} removed", name);

    Ok(())
}