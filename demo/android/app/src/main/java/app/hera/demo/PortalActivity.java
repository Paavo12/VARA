package app.hera.demo;

import android.app.Activity;
import android.content.Intent;
import android.graphics.Color;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.view.View;
import android.view.WindowInsetsController;
import android.webkit.ValueCallback;
import android.webkit.WebChromeClient;
import android.webkit.WebResourceError;
import android.webkit.WebResourceRequest;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.widget.Toast;

import java.net.HttpURLConnection;
import java.net.URL;

/**
 * Full-screen WebView host for one VARA portal.
 *
 * On launch it looks for the demo server (adb-reversed localhost, the emulator's host alias,
 * then the PC's Wi-Fi address) so this phone stays in sync with every other screen. If none
 * answers, it falls back to the copy of the web app bundled in the APK, running standalone.
 */
public abstract class PortalActivity extends Activity {

    private static final int FILE_REQUEST = 42;
    private static final String OFFLINE_URL = "file:///android_asset/index.html";

    private WebView web;
    private ValueCallback<Uri[]> fileCallback;
    private boolean offline;

    /** "rider" or "driver". */
    protected abstract String view();

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        setupWindow();

        web = new WebView(this);
        web.setBackgroundColor(Color.parseColor("#F7F5FA"));
        WebSettings s = web.getSettings();
        s.setJavaScriptEnabled(true);
        s.setDomStorageEnabled(true);
        s.setTextZoom(100); // keep the phone layout intact regardless of system font size
        WebView.setWebContentsDebuggingEnabled(true);

        web.setWebViewClient(new WebViewClient() {
            @Override
            public void onReceivedError(WebView v, WebResourceRequest req, WebResourceError err) {
                // The server went away mid-demo: keep going in standalone mode.
                if (req.isForMainFrame() && !offline) loadOffline();
            }
        });
        web.setWebChromeClient(new WebChromeClient() {
            @Override
            public boolean onShowFileChooser(WebView v, ValueCallback<Uri[]> callback, FileChooserParams params) {
                if (fileCallback != null) fileCallback.onReceiveValue(null);
                fileCallback = callback;
                try {
                    startActivityForResult(params.createIntent(), FILE_REQUEST);
                } catch (Exception e) {
                    fileCallback = null;
                    return false;
                }
                return true;
            }
        });

        setContentView(web);
        if (savedInstanceState != null) web.restoreState(savedInstanceState);
        else findServerAndLoad();
    }

    private void setupWindow() {
        // Draw behind the status bar; the web app reserves that space itself.
        getWindow().setStatusBarColor(Color.TRANSPARENT);
        getWindow().setNavigationBarColor(Color.WHITE);
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            getWindow().setDecorFitsSystemWindows(false);
            WindowInsetsController c = getWindow().getInsetsController();
            if (c != null) {
                int light = WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS | WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS;
                c.setSystemBarsAppearance(light, light);
            }
        } else {
            getWindow().getDecorView().setSystemUiVisibility(View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                    | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN | View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR);
        }
    }

    private void findServerAndLoad() {
        final String[] candidates = {"http://localhost:3100", "http://10.0.2.2:3100", BuildConfig.LAN_URL};
        new Thread(() -> {
            for (String base : candidates) {
                if (base == null || base.isEmpty()) continue;
                if (reachable(base)) {
                    runOnUiThread(() -> {
                        offline = false;
                        web.loadUrl(base + "/?view=" + view());
                    });
                    return;
                }
            }
            runOnUiThread(this::loadOffline);
        }).start();
    }

    private void loadOffline() {
        offline = true;
        web.loadUrl(OFFLINE_URL + "?view=" + view());
        Toast.makeText(this, "Demo server not found — running standalone on this phone", Toast.LENGTH_LONG).show();
    }

    private static boolean reachable(String base) {
        HttpURLConnection c = null;
        try {
            c = (HttpURLConnection) new URL(base + "/api/state").openConnection();
            c.setConnectTimeout(3000);
            c.setReadTimeout(4000);
            return c.getResponseCode() == 200;
        } catch (Exception e) {
            return false;
        } finally {
            if (c != null) c.disconnect();
        }
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        if (requestCode == FILE_REQUEST && fileCallback != null) {
            fileCallback.onReceiveValue(WebChromeClient.FileChooserParams.parseResult(resultCode, data));
            fileCallback = null;
            return;
        }
        super.onActivityResult(requestCode, resultCode, data);
    }

    @Override
    protected void onSaveInstanceState(Bundle outState) {
        super.onSaveInstanceState(outState);
        web.saveState(outState);
    }

    @Override
    @SuppressWarnings("deprecation")
    public void onBackPressed() {
        if (web.canGoBack()) web.goBack();
        else super.onBackPressed();
    }

    @Override
    protected void onDestroy() {
        web.destroy();
        super.onDestroy();
    }
}
