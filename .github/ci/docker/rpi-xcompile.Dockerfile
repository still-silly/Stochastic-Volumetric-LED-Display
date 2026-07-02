# Crosscompilation to Raspberry Pi on Raspbian/Raspberry Pi OS using system OpenCV
#
# Building this image requires `qemu-arm` to be present on the host system and the corresponding `binfmt-misc` set up (see
# e.g. https://wiki.debian.org/QemuUserEmulation, only `Installing packages` should be enough).
#
# After the successful build you will have an image configured for cross-compilation to Raspberry Pi. It will contain the
# sample build script `/usr/local/bin/cargo-xbuild` that you can check for the correct environment setup and the specific
# command line arguments to use when crosscompiling the project inside the container created from that image.

# Stage 0: Build sysroot and compile OpenCV on ARMHF
FROM balenalib/rpi-raspbian:latest AS rpi-sysroot

RUN set -xeu && \
    apt-get update && \
    DEBIAN_FRONTEND=noninteractive apt-get dist-upgrade -y && \
    DEBIAN_FRONTEND=noninteractive apt-get install -y symlinks && \
    apt-get autoremove -y --purge && \
    apt-get -y autoclean

# error: undefined symbol: _dl_pagesize (and __pointer_chk_guard_local)
# solution: fix the rpi-root symlink /usr/lib/arm-linux-gnueabihf/libpthread.so to be relative and point to ../../../lib/arm-linux-gnueabihf/libpthread.so.0
# source: https://github.com/Azure/azure-iot-sdk-c/issues/1093
RUN set -xeu && \
    symlinks -cr /

# Specify dependencies that you need to have on rpi
RUN set -xeu && \
    DEBIAN_FRONTEND=noninteractive apt-get install -y libudev-dev libsqlite3-dev libstrophe-dev libcamera-dev pkg-config

# Build OpenCV from source for most recent version (There are breaking changes betweeen the required version and the version Debian packages)
# Install dependencies for building OpenCV
RUN sudo apt-get install -y \
    build-essential ccache cmake unzip pkg-config curl \
    libjpeg-dev libpng-dev libtiff-dev \
    libavcodec-dev libavformat-dev libswscale-dev libv4l-dev \
    libxvidcore-dev libx264-dev libjasper1 libjasper-dev \
    libgtk-3-dev libcanberra-gtk* \
    libatlas-base-dev gfortran \
    libeigen3-dev libtbb-dev \
    libglfw3-dev libgl1-mesa-dev libglu1-mesa-dev \
    python3-dev python3-numpy python-dev

RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y \
    build-essential cmake \
    libvtk6-dev \
    zlib1g-dev libjpeg-dev libopenjp2-7-dev libopenjp2-tools libwebp-dev libpng-dev libtiff5-dev libjasper-dev libopenjp2-7-dev libopenexr-dev libgdal-dev \
    libdc1394-22-dev libavcodec-dev libavformat-dev libswscale-dev libtheora-dev libprotobuf-dev libvorbis-dev \
    libxvidcore-dev libx264-dev yasm libopencore-amrnb-dev libopencore-amrwb-dev libv4l-dev libxine2-dev \
    libtbb-dev libeigen3-dev \
    python-dev python-tk python3-dev python3-tk python3-numpy \
    ant default-jdk \
    doxygen \
    unzip wget && \
    apt-get clean && rm -rf /var/lib/apt/lists/*

# Build and install OpenCV from source
RUN set -eux; \
    wget https://github.com/opencv/opencv/archive/refs/tags/4.11.0.zip && \
    wget -O opencv_contrib.zip https://github.com/opencv/opencv_contrib/archive/refs/tags/4.11.0.zip && \
    unzip 4.11.0.zip && \
    unzip opencv_contrib.zip && \
    rm 4.11.0.zip && \
    rm opencv_contrib.zip && \
    mv opencv-4.11.0 OpenCV && \
    mv opencv_contrib-4.11.0 OpenCV_contrib && \
    mkdir -p OpenCV/build && \
    cd OpenCV/build && \
    cmake .. \
            -D BUILD_CUDA_STUBS=OFF \
            -D BUILD_DOCS=OFF \
            -D BUILD_EXAMPLES=OFF \
            -D BUILD_IPP_IW=ON \
            -D BUILD_ITT=ON \
            -D BUILD_JASPER=OFF \
            -D BUILD_JAVA=OFF \
            -D BUILD_JPEG=ON \
            -D BUILD_OPENEXR=OFF \
            -D BUILD_PERF_TESTS=OFF \
            -D BUILD_PNG=OFF \
            -D BUILD_PROTOBUF=ON \
            -D BUILD_SHARED_LIBS=ON \
            -D BUILD_TBB=OFF \
            -D BUILD_TESTS=OFF \
            -D BUILD_TIFF=OFF \
            -D BUILD_WEBP=OFF \
            -D BUILD_WITH_DEBUG_INFO=OFF \
            -D BUILD_WITH_DYNAMIC_IPP=OFF \
            -D BUILD_ZLIB=OFF \
            -D BUILD_opencv_apps=OFF \
            -D BUILD_opencv_python2=OFF \
            -D BUILD_opencv_python3=OFF \
            -D CMAKE_BUILD_TYPE=Release \
            -D CMAKE_INSTALL_PREFIX=/usr/local \
            -D CV_DISABLE_OPTIMIZATION=OFF \
            -D CV_ENABLE_INTRINSICS=ON \
            -D ENABLE_CONFIG_VERIFICATION=OFF \
            -D ENABLE_FAST_MATH=OFF \
            -D ENABLE_LTO=OFF \
            -D ENABLE_PIC=ON \
            -D ENABLE_PRECOMPILED_HEADERS=OFF \
            -D INSTALL_CREATE_DISTRIB=OFF \
            -D INSTALL_C_EXAMPLES=OFF \
            -D INSTALL_PYTHON_EXAMPLES=OFF \
            -D INSTALL_TESTS=OFF \
            -D OPENCV_ENABLE_MEMALIGN=OFF \
            -D OPENCV_ENABLE_NONFREE=ON \
            -D OPENCV_FORCE_3RDPARTY_BUILD=ON \
            -D OPENCV_GENERATE_PKGCONFIG=OFF \
            -D PROTOBUF_UPDATE_FILES=OFF \
            -D WITH_1394=ON \
            -D WITH_ADE=ON \
            -D WITH_ARAVIS=OFF \
            -D WITH_CLP=OFF \
            -D WITH_CUBLAS=OFF \
            -D WITH_CUDA=OFF \
            -D WITH_CUFFT=OFF \
            -D WITH_EIGEN=ON \
            -D WITH_FFMPEG=ON \
            -D WITH_GDAL=ON \
            -D WITH_GDCM=OFF \
            -D WITH_GIGEAPI=OFF \
            -D WITH_GPHOTO2=ON \
            -D WITH_GSTREAMER=ON \
            -D WITH_GSTREAMER_0_10=OFF \
            -D WITH_GTK=OFF \
            -D WITH_GTK_2_X=OFF \
            -D WITH_HALIDE=OFF \
            -D WITH_IMGCODEC_HDcR=ON \
            -D WITH_IMGCODEC_PXM=ON \
            -D WITH_IMGCODEC_SUNRASTER=ON \
            -D WITH_INF_ENGINE=OFF \
            -D WITH_IPP=ON \
            -D WITH_ITT=ON \
            -D WITH_JASPER=OFF \
            -D WITH_JPEG=ON \
            -D WITH_LAPACK=ON \
            -D WITH_LIBV4L=OFF \
            -D WITH_MATLAB=OFF \
            -D WITH_MFX=OFF \
            -D WITH_OPENCL=OFF \
            -D WITH_OPENCLAMDBLAS=OFF \
            -D WITH_OPENCLAMDFFT=OFF \
            -D WITH_OPENCL_SVM=OFF \
            -D WITH_OPENEXR=OFF \
            -D WITH_OPENGL=ON \
            -D WITH_OPENMP=OFF \
            -D WITH_OPENNI2=OFF \
            -D WITH_OPENNI=OFF \
            -D WITH_OPENVX=OFF \
            -D WITH_PNG=ON \
            -D WITH_PROTOBUF=ON \
            -D WITH_PTHREADS_PF=ON \
            -D WITH_PVAPI=OFF \
            -D WITH_QT=ON \
            -D WITH_QUIRC=ON \
            -D WITH_TBB=ON \
            -D WITH_TIFF=ON \
            -D WITH_UNICAP=OFF \
            -D WITH_V4L=ON \
            -D WITH_VA=ON \
            -D WITH_VA_INTEL=ON \
            -D WITH_VTK=ON \
            -D WITH_WEBP=ON \
            -D WITH_XIMEA=OFF \
            -D WITH_XINE=OFF \
            -D BUILD_JPEG=ON \
            -D BUILD_OPENJPEG=ON \
            -D BUILD_PNG=ON \
            -D BUILD_SHARED_LIBS=OFF \
            -D BUILD_TIFF=ON \
            -D BUILD_WEBP=ON \
            -D BUILD_ZLIB=ON \
            -D BUILD_opencv_freetype=OFF \
            -D OPENCV_FORCE_3RDPARTY_BUILD=ON \
            -D WITH_1394=OFF \
            -D WITH_FFMPEG=OFF \
            -D WITH_FREETYPE=OFF \
            -D WITH_GDAL=OFF \
            -D WITH_GPHOTO2=OFF \
            -D WITH_GSTREAMER=OFF \
            -D WITH_GTK=ON \
            -D WITH_LAPACK=OFF \
            -D WITH_OPENGL=OFF \
            -D WITH_QT=OFF \
            -D WITH_TBB=OFF \
            -D BUILD_TBB=OFF \
            -D WITH_AVIF=OFF \
            -D WITH_IMGCODEC_AVIF=OFF \
            -DOPENCV_EXTRA_MODULES_PATH=../../OpenCV_contrib/modules && \
    make -j"$(nproc)" && \
    make install && \
    ldconfig


# Stage 1: Create the Ubuntu host image used for crosscompilation
FROM ubuntu:22.04

COPY --from=rpi-sysroot / /rpi-root

RUN set -xeu && \
    apt-get update && \
    DEBIAN_FRONTEND=noninteractive apt-get dist-upgrade -y && \
    apt-get autoremove -y --purge && \
    apt-get -y autoclean

RUN set -xeu && \
    DEBIAN_FRONTEND=noninteractive apt-get install -y clang libclang-dev lld curl git build-essential pkg-config cmake

RUN set -xeu && \
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile=minimal && \
    rm -rf /root/.rustup/tmp/* # warning: could not delete temp directory: /root/.rustup/tmp/szyc3h06vricp83o_dir

# Fix: Added relocation-model=static to avoid R_ARM_ABS32 linker errors against non-PIC system libraries
RUN set -xeu && \
    echo "[net]\ngit-fetch-with-cli = true\n[target.arm-unknown-linux-gnueabihf]\nlinker = \"clang-rpi\"\nrustflags = [\"-C\", \"relocation-model=static\"]" > /root/.cargo/config

ENV PATH="${PATH}:/root/.cargo/bin"

RUN set -xeu && \
    rustup target add arm-unknown-linux-gnueabihf

RUN echo '#!/bin/bash\n\
RPI_ROOT="/rpi-root"\n\
clang --target=arm-unknown-linux-gnueabihf -fuse-ld=lld --sysroot="$RPI_ROOT" --gcc-toolchain="$RPI_ROOT" "$@"' > /usr/local/bin/clang-rpi && chmod +x /usr/local/bin/clang-rpi

RUN ls -R /rpi-root/usr/local/include/opencv4 || echo "opencv4 headers not found"

RUN echo '#!/bin/bash\n\
RPI_ROOT="/rpi-root"\n\
export PKG_CONFIG_SYSROOT_DIR="$RPI_ROOT"\n\
export PKG_CONFIG_LIBDIR="$RPI_ROOT/usr/lib/arm-linux-gnueabihf/pkgconfig"\n\
export OPENCV_HEADER_DIR="$RPI_ROOT/usr/local/include/opencv4,$RPI_ROOT/usr/local/include/opencv4"\n\
export OPENCV_INCLUDE_PATHS="$RPI_ROOT/usr/local/include/opencv4"\n\
export OPENCV_LINK_PATHS="$RPI_ROOT/usr/local/lib,$RPI_ROOT/usr/local/lib/opencv4/3rdparty,$RPI_ROOT/usr/local/lib/arm-linux-gnueabihf"\n\
export OPENCV_LINK_LIBS=opencv_gapi,opencv_highgui,opencv_objdetect,opencv_dnn,opencv_videostab,opencv_calib3d,opencv_features2d,opencv_stitching,opencv_flann,opencv_videoio,opencv_rgbd,opencv_aruco,opencv_video,opencv_ml,opencv_imgcodecs,opencv_imgproc,opencv_core,jpeg,openjp2,gtk-3,gdk-3,pangocairo-1.0,pango-1.0,harfbuzz,atk-1.0,cairo-gobject,cairo,gdk_pixbuf-2.0,gio-2.0,gobject-2.0,glib-2.0\n\
export CC="clang-rpi"\n\
export CXX="clang-rpi"\n\
cargo build -vv --release --target arm-unknown-linux-gnueabihf' > /usr/local/bin/cargo-xbuild && chmod +x /usr/local/bin/cargo-xbuild