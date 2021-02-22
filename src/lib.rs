#![allow(non_snake_case)]

use std::ffi::CStr;
use std::ptr::{self, copy_nonoverlapping as memcpy};
use std::sync::{Arc, Mutex};

use nix::errno::Errno;
use nix::fcntl::OFlag;
use nix::sys::mman::{self, MapFlags, ProtFlags};
use nix::sys::stat::Mode;
use nix::unistd;
use std::mem;

use jni::objects::{JObject, JString};
use jni::sys::jint;
use jni::JNIEnv;

use lazy_static::lazy_static;

use widestring::WideCString;

lazy_static! {
    static ref MUMBLE_LINK: Arc<Mutex<Option<&'static mut MumbleLink>>> =
        Arc::new(Mutex::new(None));
    static ref PLUGIN_NAME: WideCString = WideCString::from_str("Minecraft").unwrap();
    static ref PLUGIN_DESCRIPTION: WideCString =
        WideCString::from_str("Mumble Link implementation for Lunar Client.").unwrap();
}

// JVM types for JNI
const MUMBLE_VEC_TYPE: &'static str = "Lcom/moonsworth/client/mumble/MumbleVec;";
const JSTRING_TYPE: &'static str = "Ljava/lang/String;";

/// A struct representation of the shared memory of the Link Plugin.
#[repr(C)]
struct MumbleLink {
    ui_version: u32,
    ui_tick: u32,

    avatar_position: [f32; 3],
    avatar_front: [f32; 3],
    avatar_top: [f32; 3],

    name: [u32; 256],

    camera_position: [f32; 3],
    camera_front: [f32; 3],
    camera_top: [f32; 3],

    identity: [u32; 256],

    context_len: u32,
    context: [u8; 256],

    description: [u32; 2048],
}

const MUMBLE_LINK_SIZE: usize = mem::size_of::<MumbleLink>();

fn init_mumble_link() -> Result<&'static mut MumbleLink, nix::Error> {
    unsafe {
        let uid = unistd::getuid();
        let shm_name = format!("/MumbleLink.{}", uid);

        let raw_fd = mman::shm_open(
            shm_name.as_str(),
            OFlag::O_RDWR,
            Mode::S_IRUSR | Mode::S_IWUSR,
        )?;

        unistd::ftruncate(raw_fd, MUMBLE_LINK_SIZE as i64)?;

        let ptr = mman::mmap(
            ptr::null_mut(),
            MUMBLE_LINK_SIZE,
            ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
            MapFlags::MAP_SHARED,
            raw_fd,
            0,
        )?;

        unistd::close(raw_fd)?;

        let mumble_link = mem::transmute(ptr);

        Ok(mumble_link)
    }
}

#[no_mangle]
pub extern "system" fn Java_com_moonsworth_client_mumble_MumbleLink_init(
    _env: JNIEnv<'static>,
    _this: JObject,
) -> jint {
    let arc = MUMBLE_LINK.clone();
    let mut mumble_link = arc.lock().unwrap();

    match init_mumble_link() {
        Ok(link) => {
            *mumble_link = Some(link);
            0
        }
        Err(e) => {
            let errno = e.as_errno().unwrap_or(Errno::UnknownErrno);
            eprintln!("errno: {}", errno);
            errno as i32
        }
    }
}

fn mumble_vec_to_float_slice(env: JNIEnv<'static>, obj: JObject) -> [f32; 3] {
    // XXX: Someone messed up the conversion from a MumbleVec object to a float[3] array in the
    // Windows DLL. They did X Z Y instead of X Y Z. I have wasted a huge amount of time on that.
    // I have already reported the issue in the lunar client Discord.
    // If anyone from the lunar client dev team are reading this. Please fix that.
    [
        env.get_field(obj, "xCoord", "D").unwrap().d().unwrap() as f32,
        env.get_field(obj, "zCoord", "D").unwrap().d().unwrap() as f32,
        env.get_field(obj, "yCoord", "D").unwrap().d().unwrap() as f32,
    ]
}

fn get_vec(env: JNIEnv<'static>, link_data: JObject, name: &'static str) -> [f32; 3] {
    mumble_vec_to_float_slice(
        env,
        env.get_field(link_data, name, MUMBLE_VEC_TYPE)
            .unwrap()
            .l()
            .unwrap(),
    )
}

fn get_widestring(env: JNIEnv<'static>, obj: JObject, name: &str) -> WideCString {
    let string = JString::from(env.get_field(obj, name, JSTRING_TYPE).unwrap().l().unwrap());
    WideCString::from_str(env.get_string(string).unwrap().to_string_lossy()).unwrap()
}

fn get_string(env: JNIEnv<'static>, obj: JObject, name: &str) -> &'static CStr {
    let string = JString::from(env.get_field(obj, name, JSTRING_TYPE).unwrap().l().unwrap());
    unsafe { CStr::from_ptr(env.get_string_utf_chars(string).unwrap()) }
}

#[no_mangle]
pub extern "system" fn Java_com_moonsworth_client_mumble_MumbleLink_update(
    env: JNIEnv<'static>,
    _this: JObject,
    link_data: JObject,
) {
    let arc = MUMBLE_LINK.clone();
    let mut lock = arc.lock().unwrap();
    let mut mumble_link = lock.as_mut().unwrap();

    unsafe {
        if mumble_link.ui_version != 2 {
            let name = PLUGIN_NAME.as_slice_with_nul();
            let description = PLUGIN_DESCRIPTION.as_slice_with_nul();
            memcpy(name.as_ptr(), mumble_link.name.as_mut_ptr(), name.len());
            memcpy(
                description.as_ptr(),
                mumble_link.description.as_mut_ptr(),
                description.len(),
            );

            mumble_link.ui_version = 2;
        }

        mumble_link.ui_tick += 1;

        mumble_link
            .avatar_position
            .copy_from_slice(&get_vec(env, link_data, "avatarPosition"));
        mumble_link
            .avatar_front
            .copy_from_slice(&get_vec(env, link_data, "avatarFront"));
        mumble_link
            .avatar_top
            .copy_from_slice(&get_vec(env, link_data, "avatarTop"));

        mumble_link
            .camera_position
            .copy_from_slice(&get_vec(env, link_data, "cameraPosition"));
        mumble_link
            .camera_front
            .copy_from_slice(&get_vec(env, link_data, "cameraFront"));
        mumble_link
            .camera_top
            .copy_from_slice(&get_vec(env, link_data, "cameraTop"));

        let player_name = get_widestring(env, link_data, "playerName");
        let player_bytes = player_name.as_slice_with_nul();
        memcpy(
            player_bytes.as_ptr(),
            mumble_link.identity.as_mut_ptr(),
            player_bytes.len(),
        );

        let context = get_string(env, link_data, "context");
        // Seems that context doesn't rely on a nul terminator
        let context_bytes = context.to_bytes();
        let context_len = context_bytes.len();
        memcpy(
            context_bytes.as_ptr(),
            mumble_link.context.as_mut_ptr(),
            context_bytes.len(),
        );
        mumble_link.context_len = context_len as u32;
    }
}
