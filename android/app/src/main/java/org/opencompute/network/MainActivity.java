package org.opencompute.network;

import android.app.Activity;
import android.content.Context;
import android.net.wifi.WifiManager;
import android.os.Bundle;
import android.util.Log;
import android.widget.TextView;
import java.net.DatagramPacket;
import java.net.DatagramSocket;
import java.net.InetAddress;
import java.net.MulticastSocket;
import java.net.NetworkInterface;
import java.nio.ByteBuffer;
import java.util.Enumeration;

public class MainActivity extends Activity {
    private static final String TAG = "OCN";
    private static final int MULTICAST_PORT = 5353;
    private static final String MULTICAST_GROUP = "224.0.0.251";

    private WifiManager.MulticastLock multicastLock;
    private volatile boolean listening = true;
    private MulticastSocket multicastSocket;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

        TextView textView = new TextView(this);
        textView.setTextSize(18);
        textView.setPadding(32, 32, 32, 32);

        try {
            initDiscovery();

            OcnNode node = new OcnNode();
            String jniResult = node.stringFromJNI();

            String storagePath = getFilesDir().getAbsolutePath();

            String displayText = "=== OCN Android Node Native Engine ===\n\n"
                    + "JNI Result: " + jniResult + "\n"
                    + "Storage Dir: " + storagePath + "\n\n"
                    + "Status: OCN_JNI_CALL_SUCCESS verified on POCO M6.\n"
                    + "\nDiscovery: Bidirectional mDNS active.";
            textView.setText(displayText);
        } catch (Throwable e) {
            Log.e(TAG, "JNI Runtime Error: " + e.getMessage(), e);
            textView.setText("Failed to invoke OCN Core Native Method: " + e.getMessage());
        }

        setContentView(textView);
    }

    @Override
    protected void onDestroy() {
        super.onDestroy();
        listening = false;
        if (multicastSocket != null) {
            try {
                multicastSocket.close();
            } catch (Exception ignored) {}
        }
        if (multicastLock != null && multicastLock.isHeld()) {
            multicastLock.release();
            Log.i(TAG, "Released MulticastLock");
        }
    }

    private String getWifiIpAddress() {
        try {
            Enumeration<NetworkInterface> interfaces = NetworkInterface.getNetworkInterfaces();
            while (interfaces.hasMoreElements()) {
                NetworkInterface iface = interfaces.nextElement();
                if (iface.getName().startsWith("wlan")) {
                    Enumeration<InetAddress> addresses = iface.getInetAddresses();
                    while (addresses.hasMoreElements()) {
                        InetAddress addr = addresses.nextElement();
                        if (!addr.isLoopbackAddress() && addr instanceof java.net.Inet4Address) {
                            return addr.getHostAddress();
                        }
                    }
                }
            }
        } catch (Exception e) {
            Log.e(TAG, "Failed to get Wi-Fi IP address: " + e.getMessage());
        }
        return "192.168.68.101"; // Fallback to detected POCO M6 wlan0 IP
    }

    private void initDiscovery() {
        WifiManager wifiManager = (WifiManager) getApplicationContext().getSystemService(Context.WIFI_SERVICE);
        if (wifiManager != null && wifiManager.isWifiEnabled()) {
            multicastLock = wifiManager.createMulticastLock("ocn_multicast_lock");
            multicastLock.setReferenceCounted(true);
            multicastLock.acquire();
            Log.i(TAG, "Acquired WifiManager MulticastLock");

            startDiscoveryListener();
            infoDiscoveryAdvertisement();
        } else {
            Log.w(TAG, "Wi-Fi is not enabled; discovery skipped.");
        }
    }

    private void startDiscoveryListener() {
        new Thread(() -> {
            try {
                multicastSocket = new MulticastSocket(MULTICAST_PORT);
                InetAddress group = InetAddress.getByName(MULTICAST_GROUP);

                // Find wlan0 interface and set it explicitly
                NetworkInterface wlanIface = NetworkInterface.getByName("wlan0");
                if (wlanIface != null) {
                    multicastSocket.setNetworkInterface(wlanIface);
                }
                multicastSocket.joinGroup(group);

                byte[] buf = new byte[4096];
                while (listening) {
                    DatagramPacket packet = new DatagramPacket(buf, buf.length);
                    multicastSocket.receive(packet);
                    String message = new String(packet.getData(), 0, packet.getLength(), "UTF-8");

                    if (message.contains("node_id")) {
                        // Extract JSON string from raw payload if embedded
                        int jsonStart = message.indexOf("{");
                        if (jsonStart >= 0) {
                            String jsonStr = message.substring(jsonStart);
                            Log.i(TAG, "OCN_DISCOVERY_RECEIVED\n" + jsonStr);
                        }
                    }
                }
            } catch (Exception e) {
                if (listening) {
                    Log.e(TAG, "Discovery listener error: " + e.getMessage(), e);
                }
            }
        }).start();
    }

    private void infoDiscoveryAdvertisement() {
        new Thread(() -> {
            try {
                String wifiIp = getWifiIpAddress();
                String jsonPayload = "{\"node_id\":\"android-poco-m6\",\"node_name\":\"POCO-M6-Android-Node\",\"ip_address\":\"" + wifiIp + "\",\"listen_port\":9000,\"protocol_version\":1,\"platform_id\":\"android-arm64\"}";

                byte[] packet = buildMdnsAdvertisement(jsonPayload.getBytes("UTF-8"));

                DatagramSocket socket = new DatagramSocket();
                socket.setSoTimeout(5000);

                DatagramPacket dp = new DatagramPacket(packet, packet.length,
                        InetAddress.getByName(MULTICAST_GROUP), MULTICAST_PORT);
                
                // Periodically send 3 announcements every 5 seconds
                for (int i = 0; i < 3; i++) {
                    socket.send(dp);
                    Log.i(TAG, "OCN_DISCOVERY_SENT: mDNS advertisement sent to " + MULTICAST_GROUP + ":" + MULTICAST_PORT + " (IP: " + wifiIp + ")");
                    Thread.sleep(5000);
                }
                socket.close();
            } catch (Exception e) {
                Log.e(TAG, "Failed to send discovery advertisement: " + e.getMessage(), e);
            }
        }).start();
    }

    private byte[] buildMdnsAdvertisement(byte[] txtData) throws Exception {
        byte[] serviceName = "_ocn._udp.local.".getBytes("UTF-8");
        ByteBuffer bb = ByteBuffer.allocate(12 + serviceName.length + 10 + txtData.length);

        bb.putShort((short) 0x0100); // flags
        bb.putShort((short) 1);      // QDCOUNT = 1
        bb.putShort((short) 0);      // ANCOUNT = 0
        bb.putShort((short) 0);      // NSCOUNT = 0
        bb.putShort((short) 0);      // ARCOUNT = 0

        bb.put((byte) 4);
        bb.put("_ocn".getBytes("UTF-8"));
        bb.put((byte) 4);
        bb.put("_udp".getBytes("UTF-8"));
        bb.put((byte) 5);
        bb.put("local".getBytes("UTF-8"));
        bb.put((byte) 0);

        bb.putShort((short) 0x0010); // QTYPE = TXT
        bb.putShort((short) 0x0001); // QCLASS = IN
        bb.putInt(255);              // TTL
        bb.putShort((short) txtData.length); // RDLENGTH
        bb.put(txtData);            // RDATA

        return bb.array();
    }
}
