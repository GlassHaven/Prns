use core::cell::Cell;

use embassy_sync::blocking_mutex::{raw::CriticalSectionRawMutex, Mutex};
use embassy_sync::signal::Signal;
use personal_rns::interfaces::usb_auto::{
    UsbControllerEnrollment, UsbControllerEnrollmentBusy, UsbControllerEnrollmentStatus,
};
use personal_rns::runtime::RemoteControlControllerGrantControl;
use personal_rns::usb_auto::WebUsbControllerEnrollment;

static REQUEST: Signal<CriticalSectionRawMutex, UsbControllerEnrollment> = Signal::new();
static STATUS: Mutex<CriticalSectionRawMutex, Cell<UsbControllerEnrollmentStatus>> =
    Mutex::new(Cell::new(UsbControllerEnrollmentStatus::Idle));

pub fn webusb_enrollment(
    identity: &personal_rns::remote_control::RemoteControlNodeIdentitySecrets,
) -> WebUsbControllerEnrollment {
    static TARGET_PUBLIC_KEY: static_cell::StaticCell<
        [u8; prns_core::identity::IDENTITY_PUBLIC_KEY_LEN],
    > = static_cell::StaticCell::new();
    WebUsbControllerEnrollment::Supported {
        request,
        status,
        target_public_key: TARGET_PUBLIC_KEY.init_with(|| {
            identity
                .identities()
                .target()
                .public_keys()
                .public_key_bytes()
        }),
    }
}

fn status() -> UsbControllerEnrollmentStatus {
    STATUS.lock(Cell::get)
}

fn request(enrollment: UsbControllerEnrollment) -> Result<(), UsbControllerEnrollmentBusy> {
    STATUS.lock(|status| {
        if matches!(status.get(), UsbControllerEnrollmentStatus::Pending { .. }) {
            return Err(UsbControllerEnrollmentBusy);
        }
        status.set(UsbControllerEnrollmentStatus::Pending {
            transaction: enrollment.transaction,
        });
        REQUEST.signal(enrollment);
        Ok(())
    })
}

pub async fn run(handle: impl RemoteControlControllerGrantControl) -> ! {
    loop {
        let enrollment = REQUEST.wait().await;
        let transaction = enrollment.transaction;
        // The node restores retained authorizations before servicing this request.
        // Its grant API settles only after the journal commit becomes durable.
        let outcome = match handle
            .set_remote_control_controller_grant(enrollment.grant)
            .await
        {
            Ok(_) => UsbControllerEnrollmentStatus::Saved { transaction },
            Err(_) => UsbControllerEnrollmentStatus::Failed { transaction },
        };
        STATUS.lock(|status| status.set(outcome));
    }
}
