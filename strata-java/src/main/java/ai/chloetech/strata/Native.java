package ai.chloetech.strata;

import java.io.IOException;
import java.io.InputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.nio.file.StandardCopyOption;
import java.util.Locale;
import java.util.Map;

/**
 * The native library, {@code libstrata_jni} (the Rust crate in this
 * directory), and its two functions.
 *
 * <p>It is loaded from, in order: the path in the system property
 * {@code strata.library} (or the environment variable
 * {@code STRATA_JNI_LIBRARY}); the jar, which holds it under
 * {@code ai/chloetech/strata/native/<os>-<arch>/} for the platform it was
 * built on, unpacked into a temporary directory ({@code strata.tmpdir}, or
 * {@code java.io.tmpdir}, which must allow running what it holds); and
 * {@code java.library.path}.
 */
final class Native {
    private Native() {}

    /** The version of the calls and of {@link Codec}; the library's must match. */
    static final long ABI = 1;

    /** Runs {@code method} on the database {@code handle} (0: none). */
    static native byte[] call(long handle, String method, byte[] args);

    /** Frees a handle the library gave, closing its database if still open. */
    static native void free(long handle);

    /** The platform's directory in the jar: linux-x86_64, macos-aarch64, ... */
    static String platform() {
        String os = System.getProperty("os.name", "").toLowerCase(Locale.ROOT);
        String arch = System.getProperty("os.arch", "").toLowerCase(Locale.ROOT);
        os = os.startsWith("mac") || os.startsWith("darwin") ? "macos" : os.startsWith("linux") ? "linux" : os.startsWith("windows") ? "windows" : os.replace(' ', '_');
        arch = arch.equals("amd64") || arch.equals("x86_64") ? "x86_64" : arch.equals("aarch64") || arch.equals("arm64") ? "aarch64" : arch;
        return os + "-" + arch;
    }

    static {
        load();
        Object v = Codec.decode(call(0, "version", Codec.encode(null)));
        Object abi = v instanceof Map ? ((Map<?, ?>) v).get("abi") : null;
        if (!Long.valueOf(ABI).equals(abi)) {
            throw new UnsatisfiedLinkError("Strata's native library speaks version " + abi + " of its calls, and this jar " + ABI + ": build them from the same checkout");
        }
    }

    private static void load() {
        String explicit = System.getProperty("strata.library");
        if (explicit == null || explicit.isEmpty()) {
            explicit = System.getenv("STRATA_JNI_LIBRARY");
        }
        if (explicit != null && !explicit.isEmpty()) {
            System.load(Paths.get(explicit).toAbsolutePath().toString());
            return;
        }
        String name = System.mapLibraryName("strata_jni");
        String resource = "native/" + platform() + "/" + name;
        try (InputStream in = Native.class.getResourceAsStream(resource)) {
            if (in != null) {
                // A file to load: removed as soon as it is loaded (the mapping
                // stays), so that no copy is left behind, even after a crash.
                String tmp = System.getProperty("strata.tmpdir");
                Path dir = tmp == null || tmp.isEmpty() ? Files.createTempDirectory("strata-jni") : Files.createTempDirectory(Paths.get(tmp), "strata-jni");
                Path lib = dir.resolve(name);
                try {
                    Files.copy(in, lib, StandardCopyOption.REPLACE_EXISTING);
                    System.load(lib.toString());
                } finally {
                    if (!Files.deleteIfExists(lib) || !Files.deleteIfExists(dir)) {
                        lib.toFile().deleteOnExit();
                        dir.toFile().deleteOnExit();
                    }
                }
                return;
            }
        } catch (IOException e) {
            UnsatisfiedLinkError u = new UnsatisfiedLinkError("could not unpack Strata's native library " + resource + ": " + e);
            u.initCause(e);
            throw u;
        }
        try {
            System.loadLibrary("strata_jni");
        } catch (UnsatisfiedLinkError e) {
            UnsatisfiedLinkError u = new UnsatisfiedLinkError("Strata's native library is not in this jar for " + platform() + ", nor on java.library.path: build the jar on this platform (mvn package in strata-java), or name the library with -Dstrata.library=/path/to/" + name);
            u.initCause(e);
            throw u;
        }
    }
}
