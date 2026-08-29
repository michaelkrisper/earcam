#!/bin/sh
# Minimal Gradle wrapper launcher: boots gradle-wrapper.jar, which downloads and
# runs the distribution pinned in gradle/wrapper/gradle-wrapper.properties.
APP_HOME=$(cd "$(dirname "$0")" && pwd)
exec "${JAVA_HOME:+$JAVA_HOME/bin/}java" \
    -classpath "$APP_HOME/gradle/wrapper/gradle-wrapper.jar" \
    org.gradle.wrapper.GradleWrapperMain "$@"
