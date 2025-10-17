import axios from "axios";

const BASE_URL = "http://localhost:3001";

// Type definitions for the simplified VF Node API
interface CoinflipRequest {
  user_seed: string;
  timestamp: number; // Unix timestamp in seconds
}

interface VrfProof {
  seed_commitment: string;
  vrf_output: string;
  signature: string;
}

interface CoinflipResponse {
  node_id: string;
  heads: boolean;
  proof: VrfProof;
  timestamp: number;
  processing_time_ms: number;
}

interface HealthResponse {
  status: string;
  service: string;
  version: string;
  runtime: string;
  timestamp: number;
}

interface NodeInfo {
  node_pubkey: string;
  service: string;
  version: string;
  supported_games: string[];
  max_concurrent: number;
  features: string[];
}

async function testHealth(): Promise<void> {
  try {
    console.log("🏥 Testing health endpoint...");
    const response = await axios.get<HealthResponse>(`${BASE_URL}/health`);
    console.log("✅ Health check passed:", response.data);
  } catch (error) {
    console.error("❌ Health check failed:", error);
    throw error;
  }
}

async function testNodeInfo(): Promise<NodeInfo> {
  try {
    console.log("ℹ️  Testing node info endpoint...");
    const response = await axios.get<NodeInfo>(`${BASE_URL}/info`);
    console.log("✅ Node info received:", response.data);
    return response.data;
  } catch (error) {
    console.error("❌ Node info failed:", error);
    throw error;
  }
}

async function testCoinflip(userSeed: string): Promise<CoinflipResponse> {
  try {
    console.log(`🪙 Testing coinflip with seed: "${userSeed}"`);

    const request: CoinflipRequest = {
      user_seed: userSeed,
      timestamp: Math.floor(Date.now() / 1000),
    };

    const startTime = Date.now();
    const response = await axios.post<CoinflipResponse>(
      `${BASE_URL}/coinflip`,
      request
    );
    const endTime = Date.now();

    const result = response.data;
    const roundTripTime = endTime - startTime;

    console.log("✅ Coinflip result:", {
      node_id: result.node_id.substring(0, 12) + "...",
      heads: result.heads,
      result: result.heads ? "HEADS" : "TAILS",
      processing_time_ms: result.processing_time_ms,
      round_trip_ms: roundTripTime,
      timestamp: new Date(result.timestamp * 1000).toISOString(),
    });

    return result;
  } catch (error) {
    console.error("❌ Coinflip failed:", error);
    throw error;
  }
}

async function stressTest(numRequests: number): Promise<void> {
  console.log(`\n🚀 Starting stress test with ${numRequests} requests...`);

  const startTime = Date.now();
  const promises: Promise<CoinflipResponse>[] = [];

  for (let i = 0; i < numRequests; i++) {
    const seed = `stress_test_${i}_${Date.now()}`;
    promises.push(testCoinflip(seed));
  }

  try {
    const results = await Promise.all(promises);
    const endTime = Date.now();

    const totalTime = endTime - startTime;
    const avgProcessingTime =
      results.reduce((sum, r) => sum + r.processing_time_ms, 0) /
      results.length;
    const requestsPerSecond = (numRequests / totalTime) * 1000;

    console.log("\n📊 Stress Test Results:");
    console.log(`   Total requests: ${numRequests}`);
    console.log(`   Total time: ${totalTime}ms`);
    console.log(`   Requests/second: ${requestsPerSecond.toFixed(2)}`);
    console.log(
      `   Average processing time: ${avgProcessingTime.toFixed(2)}ms`
    );
    console.log(`   Success rate: 100%`);

    // Distribution analysis
    const heads = results.filter((r) => r.heads).length;
    const tails = results.length - heads;
    console.log(
      `   Distribution: ${heads} heads (${(
        (heads / results.length) *
        100
      ).toFixed(1)}%), ${tails} tails (${(
        (tails / results.length) *
        100
      ).toFixed(1)}%)`
    );
  } catch (error) {
    console.error("❌ Stress test failed:", error);
    throw error;
  }
}

async function main() {
  console.log("🎯 VF Node Test Client - Performance Optimized Version\n");

  try {
    // Basic functionality tests
    await testHealth();
    await testNodeInfo();

    // Single coinflip tests
    console.log("\n📝 Single coinflip tests:");
    await testCoinflip("test_seed_1");
    await testCoinflip("different_seed");
    await testCoinflip("another_random_seed_123");

    // Performance tests
    console.log("\n⚡ Performance tests:");
    await stressTest(10); // Warm up
    await stressTest(100); // Medium load
    await stressTest(1000); // High load

    console.log("\n🎉 All tests completed successfully!");
  } catch (error) {
    console.error("\n💥 Test suite failed:", error);
    process.exit(1);
  }
}

// Run the tests
main().catch(console.error);
