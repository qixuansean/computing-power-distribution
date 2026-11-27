package org.opencompute.network;

public class OcnNode {
    static {
        System.loadLibrary("ocn_core");
    }

    public native String stringFromJNI();

    public static void main(String[] args) {
        OcnNode node = new OcnNode();
        System.out.println("OCN Android Node Initializing...");
        System.out.println("JNI Engine Call Output: " + node.stringFromJNI());
    }
}
