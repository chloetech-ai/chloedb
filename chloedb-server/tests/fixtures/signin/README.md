# Google sign-in test key

`key.pem` is an RSA 2048 key made with
`openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048` for
`tests/signin.rs` alone: it signs the ID tokens of those tests, and a fake
of Google's published keys (`/certs`) serves its public half. It is not
Google's, and no server takes it outside those tests.
