#!/usr/bin/env python3

from contextlib import contextmanager
from concurrent.futures import ThreadPoolExecutor
import os
import sys
from threading import Event
from time import sleep, perf_counter
import traceback

import docker
import requests


URL_PREFIX = "http://127.0.0.1:8080"
HTTP_CHECK_FUNCS = []


def test_image(image):
    dockerc = docker.from_env()
    with run_container(dockerc, image) as container, requests.Session() as session:
        print(f"Container started, to tail its logs: docker logs -f -t {container.id}")
        start = perf_counter()
        wait_for_container_ready(session)
        start_duration = perf_counter() - start
        print(f"Service has started responding in {start_duration}s.")

        check(logs_on_startup, container)
        check(logs_each_request, container, session)
        perform_http_checks(session)

        with collect_stats(container):
            pass  # TODO: run stress tests


@contextmanager
def run_container(dockerc: docker.DockerClient, image):
    # Give only 1s of CPU-core-time per each wall clock second. (can still run in parallel). Lets the rest of the
    # system breathe and better simulates Kubernetes environment (which uses the same method of capping CPU).
    nano_cpus = 10**9
    # 512 MB should be a conservative limit of something called a microservice. Setting swap to same value to disable.
    mem_limit = "512m"
    environment = {key: os.environ[key] for key in ('GOOUT_ELASTIC_HOST', 'GOOUT_ELASTIC_PORT')}
    cpuset_cpus = "0-3"  # Assign 4 logical CPUs to the container to simulate our real cluster.

    container = dockerc.containers.run(
        image, auto_remove=True, detach=True, nano_cpus=nano_cpus, mem_limit=mem_limit, memswap_limit=mem_limit,
        ports={8080: 8080}, environment=environment, cpuset_cpus=cpuset_cpus)
    try:
        yield container
    finally:
        container.kill()


def wait_for_container_ready(session):
    timeout = 15
    last_exc = None
    for _ in range(timeout * 100):  # We wait one hundredth of a second.
        try:
            return session.get(URL_PREFIX)
        except requests.exceptions.ConnectionError as e:
            last_exc = e
            sleep(0.01)
    raise AssertionError(f"Failed to connect to {URL_PREFIX} in {timeout}s: {last_exc}. Does it listen on 0.0.0.0?")


def check(func, *args):
    """Perform a single check and print its result."""
    print(f"{func.__doc__}: ", end="")
    try:
        func(*args)
        print("Good")
    except AssertionError as e:
        failed_line = traceback.extract_tb(sys.exc_info()[2])[-1].line  # magic to extract the "assert line"
        print(f"Bad: {failed_line}: {e}")


def logs_on_startup(container):
    """Service logs a message containing 8080 (used port) on startup"""
    out = container.logs().decode()
    assert "8080" in out, f"got {len(out.splitlines())} lines of log: \n{out}"


def logs_each_request(container, session):
    """Service logs every request, message contains url path"""
    path = "/blablaGOGOthisIsCanaryValue"
    session.get(URL_PREFIX + path)
    out = container.logs().decode()
    assert path in out, f"got {len(out.splitlines())} lines of log: \n{out}"


def test_local():
    with requests.Session() as session:
        perform_http_checks(session)


def perform_http_checks(session):
    for func in HTTP_CHECK_FUNCS:
        check(func, session)


def http_check(func):
    """Simple decorator to mark a function as an HTTP check."""
    HTTP_CHECK_FUNCS.append(func)
    return func


@http_check
def http_check_root(session: requests.Session):
    """HTTP GET / returns 200 or 404"""
    res = session.get(URL_PREFIX + "/")
    assert res.status_code in (200, 404), (res, res.text)


@http_check
def http_check_nonexistent_path(session: requests.Session):
    """HTTP GET /fnhjkdniudsancyne returns 404"""
    res = session.get(URL_PREFIX + "/fnhjkdniudsancyne")
    assert res.status_code == 404, (res, res.text)


@http_check
def http_check_no_params(session: requests.Session):
    """HTTP GET /city/v1/get returns 400 with error JSON with message"""
    res = session.get(URL_PREFIX + "/city/v1/get")
    assert_error_reply(res, 400)


@http_check
def http_check_just_id_param(session: requests.Session):
    """HTTP GET /city/v1/get?id=123 returns 400 with error JSON with message"""
    res = session.get(URL_PREFIX + "/city/v1/get?id=123")
    assert_error_reply(res, 400)


@http_check
def http_check_invalid_id(session: requests.Session):
    """HTTP GET /city/v1/get?id=blabla&language=cs returns 400 with error JSON with message"""
    res = session.get(URL_PREFIX + "/city/v1/get?id=blabla&language=cs")
    assert_error_reply(res, 400)


@http_check
def http_check_just_language_param(session: requests.Session):
    """HTTP GET /city/v1/get?language=cs returns 400 with error JSON with message"""
    res = session.get(URL_PREFIX + "/city/v1/get?language=cs")
    assert_error_reply(res, 400)


@http_check
def http_check_nonexistent_city_id(session: requests.Session):
    """HTTP GET /city/v1/get?id=123&language=cs returns 404 (this does not exist) with error JSON with message"""
    res = session.get(URL_PREFIX + "/city/v1/get?id=123&language=cs")
    assert_error_reply(res, 404)


def assert_error_reply(res: requests.Response, expected_code):
    assert res.status_code == expected_code, (expected_code, res, res.text)
    assert res.headers['content-type'] == 'application/json', res.headers
    json = res.json()
    assert 'message' in json, json


@http_check
def http_check_plzen_cs(session: requests.Session):
    """HTTP GET /city/v1/get?id=101748111&language=cs returns 200 and correct object"""
    res = session.get(URL_PREFIX + "/city/v1/get?id=101748111&language=cs")
    assert_city_reply(res, 101748111, "Plzeň", "Plzeňský kraj", "CZ")


@http_check
def http_check_brno_de(session: requests.Session):
    """HTTP GET /city/v1/get?id=101748109&language=de returns 200 and correct object"""
    res = session.get(URL_PREFIX + "/city/v1/get?id=101748109&language=de")
    assert_city_reply(res, 101748109, "Brünn", "Südmährische Region", "CZ")


@http_check
def http_check_graz_cs_extra_param(session: requests.Session):
    """HTTP GET /city/v1/get?id=1108839329&language=cs&extra=paramShouldBeIgnored returns 200 and correct object"""
    res = session.get(URL_PREFIX + "/city/v1/get?id=1108839329&language=cs&extra=paramShouldBeIgnored")
    assert_city_reply(res, 1108839329, "Štýrský Hradec", "Štýrsko", "AT")


def assert_city_reply(res: requests.Response, expected_id, expected_city, expected_region, expected_country):
    assert res.status_code == 200, (res, res.text)
    assert res.headers['content-type'] == 'application/json', res.headers
    json = res.json()
    assert json.keys() == {'countryISO', 'id', 'isFeatured', 'name', 'regionName'}, json
    assert json['countryISO'] == expected_country, (expected_country, json)
    assert json['id'] == expected_id, (expected_id, json)
    assert type(json['isFeatured']) == bool, json  # Not yet in Elastic, check just type
    assert json['name'] == expected_city, (expected_city, json)
    assert json['regionName'] == expected_region, (expected_region, json)


@contextmanager
def collect_stats(container):
    thread_executor = ThreadPoolExecutor(max_workers=1)
    event = Event()
    collect_stats_future = thread_executor.submit(collect_stats_thread, container, event)

    try:
        yield
    finally:
        event.set()
        assert collect_stats_future.result(2) is None  # catch possible exception from the thread


def collect_stats_thread(container, event: Event):
    for stats in container.stats(decode=True):
        # TODO: parse "read" date
        cpu_total_ms = stats['cpu_stats']['cpu_usage']['total_usage'] / 1000**2
        mem_usage_mb = stats['memory_stats']['usage'] / 1024**2
        mem_max_usage_mb = stats['memory_stats']['max_usage'] / 1024**2
        print(f'cpu_total_ms: {cpu_total_ms} mem_usage_mb: {mem_usage_mb} mem_max_usage_mb: {mem_max_usage_mb}')
        if event.is_set():
            break


if __name__ == '__main__':
    if len(sys.argv) != 2:
        print(f"Usage: {sys.argv[0]} (docker-image/with-optional:tag | --local)")
        exit(1)
    arg = sys.argv[1]
    if arg == '--local':
        test_local()
    else:
        test_image(arg)
