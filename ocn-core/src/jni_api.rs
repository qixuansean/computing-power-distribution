#[cfg(feature = "android-jni")]
use jni::JNIEnv;
#[cfg(feature = "android-jni")]
use jni::objects::JClass;
#[cfg(feature = "android-jni")]
use jni::sys::jstring;

#[cfg(feature = "android-jni")]
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_org_opencompute_network_OcnNode_stringFromJNI<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jstring {
    let output = env.new_string("OCN Core Android Native Engine initialized successfully.")
        .expect("Couldn't create java string!");
    output.into_raw()
}
