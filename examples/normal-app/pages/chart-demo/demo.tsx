import { EChart } from "@kbox-labs/react-echarts";

export function Demo() {
  return (
    <main>
      <div className="w-full h-[600px]">
        <EChart
          className="h-full"
          renderer={"canvas"}
          onClick={() => console.log("clicked!")}
          xAxis={{
            type: "category",
          }}
          yAxis={{
            type: "value",
            boundaryGap: [0, "30%"],
          }}
          series={[
            {
              type: "line",
              data: [
                ["2022-10-17", 300],
                ["2022-10-18", 100],
              ],
            },
          ]}
        />
      </div>
    </main>
  );
}
