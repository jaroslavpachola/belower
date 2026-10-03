Note the instructions use `podman` instead of `docker` because at time of
writing, docker doesn't yet have support for cgroup2.

There is no prebuilt belower image yet; build one from the repository's
Dockerfile:

```shell
$ git clone https://github.com/jaroslavpachola/belower.git ~/dev/belower
<...>

$ cd ~/dev/belower

$ podman build -t belower .
<...>

$ podman run --privileged --cgroupns=host --pid=host -it belower
```
