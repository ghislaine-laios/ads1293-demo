import { EChart } from "@kbox-labs/react-echarts";
import useWebSocket, { ReadyState } from "react-use-websocket";
import { push_data_url } from "../../src/config";
import { useEffect, useReducer, useRef, useState } from "react";
import { Euler, Quaternion } from "three";

export function Demo() {
  const data = useData();

  const [ecg_data, _] = useEcgData(data);

  let quaternion: Quaternion | null = data
    ? new Quaternion(
        data[1].quaternion[0],
        data[1].quaternion[1],
        data[1].quaternion[2],
        data[1].quaternion[3]
      )
    : null;

  let euler = quaternion ? new Euler().setFromQuaternion(quaternion!) : null;
  // if (euler) {
  //   euler.setFromQuaternion(quaternion!);
  // }

  const xLabels = useRef([] as number[]);
  if (xLabels.current.length == 0) {
    xLabels.current = new Array(DATA_ARR_LEN).fill(0) as number[];
    xLabels.current = xLabels.current.map((_value, index) => {
      return index;
    });
  }

  return (
    <main>
      <div className="w-full h-[1000px]">
        <EChart
          className="h-full"
          renderer={"canvas"}
          onClick={() => console.log("clicked!")}
          xAxis={{
            data: xLabels.current,
          }}
          yAxis={{
            type: "value",
            // boundaryGap: ["20%", "20%"],
            // scale: true,
            max: 6200000,
            min: 5960000
          }}
          series={[
            {
              type: "line",
              smooth: true,
              data: ecg_data as number[],
              showSymbol: false,
            },
          ]}
        />
      </div>
      {euler ? (
        <div>
          Orientation: X = {radians_to_degrees(euler.x)}, Y ={" "}
          {radians_to_degrees(euler.y)}, Z = {radians_to_degrees(euler.z)}
        </div>
      ) : null}
    </main>
  );
}

function useLogMsg(lastJsonMessage: any, readyState: ReadyState) {
  const debug_counter = useRef(0);
  useEffect(() => {
    debug_counter.current++;
    if (debug_counter.current >= 180) {
      console.debug(lastJsonMessage, readyState);
      debug_counter.current = 0;
    }
  }, [lastJsonMessage, readyState]);
}

type Data = [
  number,
  { id: number; ecg: number; quaternion: [number, number, number, number] },
];

function useData(): Data | null {
  const { lastJsonMessage, readyState } = useWebSocket<Data>(push_data_url, {
    heartbeat: {
      message: "ping",
      returnMessage: "pong",
      timeout: 3000,
      interval: 1000,
    },
  });

  useLogMsg(lastJsonMessage, readyState);

  return lastJsonMessage;
}

const DATA_ARR_LEN = 60 * 5;
const ECG_LINE_WIDTH = 10;

function useEcgData(data: Data | null): [(number | null)[], number] {
  const data_arr = useRef(null as unknown as (number | null)[]);

  function clearDataArr() {
    data_arr.current = new Array(DATA_ARR_LEN).fill(null) as (number | null)[];
  }

  function clearRange(start: number, end: number) {
    for (let i = start; i < end; i++) {
      data_arr.current[i] = null;
    }
  }

  function updateDataArr(id: number, value: number) {
    let left_index = id % DATA_ARR_LEN;
    let right_index = (left_index + ECG_LINE_WIDTH) % DATA_ARR_LEN;

    if (right_index > left_index) {
      clearRange(left_index + 1, right_index);
    } else {
      clearRange(left_index, data_arr.current.length);
      clearRange(0, right_index);
    }

    data_arr.current[left_index] = value;
  }

  const [nonce, increaseNonce] = useReducer((v) => v++, 0);
  const [currentDataProcessorID, setCurrentDataProcessorID] = useState(-1);

  useEffect(() => {
    if (!data) return;
    if (currentDataProcessorID != data[0]) {
      clearDataArr();
      setCurrentDataProcessorID(data[0]);
    }

    updateDataArr(data[1].id, data[1].ecg);
    increaseNonce();
  }, [data]);

  return [data_arr.current, nonce];
}

// Define a function named radians_to_degrees that converts radians to degrees.
function radians_to_degrees(radians: number) {
  // Store the value of pi.
  var pi = Math.PI;
  // Multiply radians by 180 divided by pi to convert to degrees.
  return radians * (180 / pi);
}
