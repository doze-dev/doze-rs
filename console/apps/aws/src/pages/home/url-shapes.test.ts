import { expect, test } from "bun:test";
import { urlShapes } from "./url-shapes";

test("every shape is minted from the address asked with", () => {
  const s = urlShapes("http://127.0.0.1:4566", "123456789012", "us-east-1");
  expect(s[0]?.urls[0]?.url).toBe("http://127.0.0.1:4566/123456789012/pick-lists");
  expect(s[2]?.urls[0]?.url).toBe("http://127.0.0.1:4566/_aws/execute-api/<id>/<stage>/");
});
test("a Function URL has the host form, at localhost for an IP, and the path form with the Region", () => {
  const u = urlShapes("http://127.0.0.1:4566", "1", "eu-west-1")[3]?.urls.map((x) => x.url);
  expect(u).toEqual(["http://<id>.lambda-url.eu-west-1.localhost:4566/", "http://127.0.0.1:4566/_aws/lambda-url/eu-west-1/<id>/"]);
});
test("a name is kept as it is", () => {
  expect(urlShapes("http://aws.larder.doze", "1", "us-east-1")[3]?.urls[0]?.url).toBe("http://<id>.lambda-url.us-east-1.aws.larder.doze/");
});
