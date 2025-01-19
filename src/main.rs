use anyhow::{Context, Result};
use clap::Parser;
use log::info;

mod models;
mod steps;
mod utils;

use models::args::MainArgs;
use models::config::Config;
use models::manifest::Manifest;
use steps::generate::Generator;
use steps::package::Packaging;
use steps::prepare::Preparator;
use utils::logging::init_logger;
use utils::sign::Signer;

fn main() -> Result<()> {
    let args: MainArgs = MainArgs::parse();
    let mut conf = Config::from_file(args.config.as_path())?;

    let level = if args.verbose {
        "trace"
    } else {
        conf.general.log_level.as_str()
    };
    init_logger(level);

    // Only validate config
    if args.test_config {
        return match conf.apply_args(&args) {
            Ok(_) => {
                info!("Config Ok!");
                Ok(())
            }
            Err(e) => {
                info!("Config invalid: {e}");
                Err(e)
            }
        };
    }

    info!("Verifying config validity...");
    conf.apply_args(&args).context("Config invalid")?;
    info!("Config Ok!");

    info!("bouf process started with the following locations set:");
    info!(" - Input dir: {}", &conf.env.input_dir.display());
    info!(" - Previous versions dir: {}", &conf.env.previous_dir.display());
    info!(" - Output dir: {}", &conf.env.output_dir.display());

    if !args.updater_data_only {
        let prep = Preparator::init(&conf);
        prep.run().context("Preparation failed")?;
    } else {
        info!("Skipped preparation, this will also disable installer/zip creation.")
    }

    let mut manifest: Option<Manifest> = None;
    if !args.packaging_only {
        // Create deltas and manifest
        info!("Creating manifest and patches...");
        let generator = Generator::init(&conf, !args.updater_data_only);
        manifest = Some(generator.run(args.skip_patches).context("Error during generator run")?);
    }

    let packager = Packaging::init(&conf);
    // Create NSIS/ZIP
    if !conf.package.installer.skip && !args.updater_data_only {
        info!("Creating Installer");
        packager.run_nsis().context("NSIS creation/signing failed")?;
    } else {
        info!("Skipping installer creation...")
    }

    if !args.updater_data_only && !conf.package.zip.skip {
        // Create PDB and install folder ZIPs
        info!("Creating zip files...");
        packager.create_zips().context("Creating zip files failed")?;
        info!("ZIP files created successfully!")
    } else if !conf.package.zip.skip {
        info!(" Skipping ZIP creation as preparation was skipped...")
    }

    if let Some(mut mf) = manifest {
        // Sign manifest if it was created
        info!("Finalising manifest...");
        let manifest_file = packager
            .finalise_manifest(&mut mf)
            .context("Finalising manifest failed")?;

        if !conf.package.updater.skip_sign {
            info!("Signing manifest...");
            let mut signer = Signer::init(conf.package.updater.private_key.as_ref());
            signer.sign_file(&manifest_file).context("Signing file failed")?;
        }
    }

    if !args.updater_data_only && conf.post.copy_to_old {
        info!("Copying install dir and PDBs to backup directory...");
        steps::post::copy_to_old(&conf).context("Copying files failed")?;
    }

    info!("*** Finished! ***");
    Ok(())
}
