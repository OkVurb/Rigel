# Generated TypeScript README
This README will guide you through the process of using the generated JavaScript SDK package for the connector `example`. It will also provide examples on how to use your generated SDK to call your Data Connect queries and mutations.

**If you're looking for the `React README`, you can find it at [`dataconnect-generated/react/README.md`](./react/README.md)**

***NOTE:** This README is generated alongside the generated SDK. If you make changes to this file, they will be overwritten when the SDK is regenerated.*

# Table of Contents
- [**Overview**](#generated-javascript-readme)
- [**Accessing the connector**](#accessing-the-connector)
  - [*Connecting to the local Emulator*](#connecting-to-the-local-emulator)
- [**Queries**](#queries)
  - [*GetUser*](#getuser)
  - [*ListUsers*](#listusers)
  - [*GetCelestialBody*](#getcelestialbody)
  - [*ListCelestialBodies*](#listcelestialbodies)
  - [*GetEvent*](#getevent)
  - [*ListEvents*](#listevents)
  - [*GetSession*](#getsession)
  - [*ListMySessions*](#listmysessions)
  - [*GetObservation*](#getobservation)
  - [*ListMyObservations*](#listmyobservations)
  - [*GetPhotoSetting*](#getphotosetting)
  - [*ListMyPhotoSettings*](#listmyphotosettings)
- [**Mutations**](#mutations)
  - [*CreateUser*](#createuser)
  - [*UpdateUser*](#updateuser)
  - [*DeleteUser*](#deleteuser)
  - [*CreateCelestialBody*](#createcelestialbody)
  - [*UpdateCelestialBody*](#updatecelestialbody)
  - [*DeleteCelestialBody*](#deletecelestialbody)
  - [*CreateEvent*](#createevent)
  - [*UpdateEvent*](#updateevent)
  - [*DeleteEvent*](#deleteevent)
  - [*CreateSession*](#createsession)
  - [*UpdateSession*](#updatesession)
  - [*DeleteSession*](#deletesession)
  - [*CreateObservation*](#createobservation)
  - [*UpdateObservation*](#updateobservation)
  - [*DeleteObservation*](#deleteobservation)
  - [*CreatePhotoSetting*](#createphotosetting)
  - [*UpdatePhotoSetting*](#updatephotosetting)
  - [*DeletePhotoSetting*](#deletephotosetting)

# Accessing the connector
A connector is a collection of Queries and Mutations. One SDK is generated for each connector - this SDK is generated for the connector `example`. You can find more information about connectors in the [Data Connect documentation](https://firebase.google.com/docs/data-connect#how-does).

You can use this generated SDK by importing from the package `@dataconnect/generated` as shown below. Both CommonJS and ESM imports are supported.

You can also follow the instructions from the [Data Connect documentation](https://firebase.google.com/docs/data-connect/web-sdk#set-client).

```typescript
import { getDataConnect } from 'firebase/data-connect';
import { connectorConfig } from '@dataconnect/generated';

const dataConnect = getDataConnect(connectorConfig);
```

## Connecting to the local Emulator
By default, the connector will connect to the production service.

To connect to the emulator, you can use the following code.
You can also follow the emulator instructions from the [Data Connect documentation](https://firebase.google.com/docs/data-connect/web-sdk#instrument-clients).

```typescript
import { connectDataConnectEmulator, getDataConnect } from 'firebase/data-connect';
import { connectorConfig } from '@dataconnect/generated';

const dataConnect = getDataConnect(connectorConfig);
connectDataConnectEmulator(dataConnect, 'localhost', 9399);
```

After it's initialized, you can call your Data Connect [queries](#queries) and [mutations](#mutations) from your generated SDK.

# Queries

There are two ways to execute a Data Connect Query using the generated Web SDK:
- Using a Query Reference function, which returns a `QueryRef`
  - The `QueryRef` can be used as an argument to `executeQuery()`, which will execute the Query and return a `QueryPromise`
- Using an action shortcut function, which returns a `QueryPromise`
  - Calling the action shortcut function will execute the Query and return a `QueryPromise`

The following is true for both the action shortcut function and the `QueryRef` function:
- The `QueryPromise` returned will resolve to the result of the Query once it has finished executing
- If the Query accepts arguments, both the action shortcut function and the `QueryRef` function accept a single argument: an object that contains all the required variables (and the optional variables) for the Query
- Both functions can be called with or without passing in a `DataConnect` instance as an argument. If no `DataConnect` argument is passed in, then the generated SDK will call `getDataConnect(connectorConfig)` behind the scenes for you.

Below are examples of how to use the `example` connector's generated functions to execute each query. You can also follow the examples from the [Data Connect documentation](https://firebase.google.com/docs/data-connect/web-sdk#using-queries).

## GetUser
You can execute the `GetUser` query using the following action shortcut function, or by calling `executeQuery()` after calling the following `QueryRef` function, both of which are defined in [dataconnect-generated/index.d.ts](./index.d.ts):
```typescript
getUser(options?: ExecuteQueryOptions): QueryPromise<GetUserData, undefined>;

interface GetUserRef {
  ...
  /* Allow users to create refs without passing in DataConnect */
  (): QueryRef<GetUserData, undefined>;
}
export const getUserRef: GetUserRef;
```
You can also pass in a `DataConnect` instance to the action shortcut function or `QueryRef` function.
```typescript
getUser(dc: DataConnect, options?: ExecuteQueryOptions): QueryPromise<GetUserData, undefined>;

interface GetUserRef {
  ...
  (dc: DataConnect): QueryRef<GetUserData, undefined>;
}
export const getUserRef: GetUserRef;
```

If you need the name of the operation without creating a ref, you can retrieve the operation name by calling the `operationName` property on the getUserRef:
```typescript
const name = getUserRef.operationName;
console.log(name);
```

### Variables
The `GetUser` query has no variables.
### Return Type
Recall that executing the `GetUser` query returns a `QueryPromise` that resolves to an object with a `data` property.

The `data` property is an object of type `GetUserData`, which is defined in [dataconnect-generated/index.d.ts](./index.d.ts). It has the following fields:
```typescript
export interface GetUserData {
  user?: {
    email: string;
    displayName: string;
  };
}
```
### Using `GetUser`'s action shortcut function

```typescript
import { getDataConnect } from 'firebase/data-connect';
import { connectorConfig, getUser } from '@dataconnect/generated';


// Call the `getUser()` function to execute the query.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await getUser();

// You can also pass in a `DataConnect` instance to the action shortcut function.
const dataConnect = getDataConnect(connectorConfig);
const { data } = await getUser(dataConnect);

console.log(data.user);

// Or, you can use the `Promise` API.
getUser().then((response) => {
  const data = response.data;
  console.log(data.user);
});
```

### Using `GetUser`'s `QueryRef` function

```typescript
import { getDataConnect, executeQuery } from 'firebase/data-connect';
import { connectorConfig, getUserRef } from '@dataconnect/generated';


// Call the `getUserRef()` function to get a reference to the query.
const ref = getUserRef();

// You can also pass in a `DataConnect` instance to the `QueryRef` function.
const dataConnect = getDataConnect(connectorConfig);
const ref = getUserRef(dataConnect);

// Call `executeQuery()` on the reference to execute the query.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await executeQuery(ref);

console.log(data.user);

// Or, you can use the `Promise` API.
executeQuery(ref).then((response) => {
  const data = response.data;
  console.log(data.user);
});
```

## ListUsers
You can execute the `ListUsers` query using the following action shortcut function, or by calling `executeQuery()` after calling the following `QueryRef` function, both of which are defined in [dataconnect-generated/index.d.ts](./index.d.ts):
```typescript
listUsers(options?: ExecuteQueryOptions): QueryPromise<ListUsersData, undefined>;

interface ListUsersRef {
  ...
  /* Allow users to create refs without passing in DataConnect */
  (): QueryRef<ListUsersData, undefined>;
}
export const listUsersRef: ListUsersRef;
```
You can also pass in a `DataConnect` instance to the action shortcut function or `QueryRef` function.
```typescript
listUsers(dc: DataConnect, options?: ExecuteQueryOptions): QueryPromise<ListUsersData, undefined>;

interface ListUsersRef {
  ...
  (dc: DataConnect): QueryRef<ListUsersData, undefined>;
}
export const listUsersRef: ListUsersRef;
```

If you need the name of the operation without creating a ref, you can retrieve the operation name by calling the `operationName` property on the listUsersRef:
```typescript
const name = listUsersRef.operationName;
console.log(name);
```

### Variables
The `ListUsers` query has no variables.
### Return Type
Recall that executing the `ListUsers` query returns a `QueryPromise` that resolves to an object with a `data` property.

The `data` property is an object of type `ListUsersData`, which is defined in [dataconnect-generated/index.d.ts](./index.d.ts). It has the following fields:
```typescript
export interface ListUsersData {
  users: ({
    id: UUIDString;
    displayName: string;
  } & User_Key)[];
}
```
### Using `ListUsers`'s action shortcut function

```typescript
import { getDataConnect } from 'firebase/data-connect';
import { connectorConfig, listUsers } from '@dataconnect/generated';


// Call the `listUsers()` function to execute the query.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await listUsers();

// You can also pass in a `DataConnect` instance to the action shortcut function.
const dataConnect = getDataConnect(connectorConfig);
const { data } = await listUsers(dataConnect);

console.log(data.users);

// Or, you can use the `Promise` API.
listUsers().then((response) => {
  const data = response.data;
  console.log(data.users);
});
```

### Using `ListUsers`'s `QueryRef` function

```typescript
import { getDataConnect, executeQuery } from 'firebase/data-connect';
import { connectorConfig, listUsersRef } from '@dataconnect/generated';


// Call the `listUsersRef()` function to get a reference to the query.
const ref = listUsersRef();

// You can also pass in a `DataConnect` instance to the `QueryRef` function.
const dataConnect = getDataConnect(connectorConfig);
const ref = listUsersRef(dataConnect);

// Call `executeQuery()` on the reference to execute the query.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await executeQuery(ref);

console.log(data.users);

// Or, you can use the `Promise` API.
executeQuery(ref).then((response) => {
  const data = response.data;
  console.log(data.users);
});
```

## GetCelestialBody
You can execute the `GetCelestialBody` query using the following action shortcut function, or by calling `executeQuery()` after calling the following `QueryRef` function, both of which are defined in [dataconnect-generated/index.d.ts](./index.d.ts):
```typescript
getCelestialBody(options?: ExecuteQueryOptions): QueryPromise<GetCelestialBodyData, undefined>;

interface GetCelestialBodyRef {
  ...
  /* Allow users to create refs without passing in DataConnect */
  (): QueryRef<GetCelestialBodyData, undefined>;
}
export const getCelestialBodyRef: GetCelestialBodyRef;
```
You can also pass in a `DataConnect` instance to the action shortcut function or `QueryRef` function.
```typescript
getCelestialBody(dc: DataConnect, options?: ExecuteQueryOptions): QueryPromise<GetCelestialBodyData, undefined>;

interface GetCelestialBodyRef {
  ...
  (dc: DataConnect): QueryRef<GetCelestialBodyData, undefined>;
}
export const getCelestialBodyRef: GetCelestialBodyRef;
```

If you need the name of the operation without creating a ref, you can retrieve the operation name by calling the `operationName` property on the getCelestialBodyRef:
```typescript
const name = getCelestialBodyRef.operationName;
console.log(name);
```

### Variables
The `GetCelestialBody` query has no variables.
### Return Type
Recall that executing the `GetCelestialBody` query returns a `QueryPromise` that resolves to an object with a `data` property.

The `data` property is an object of type `GetCelestialBodyData`, which is defined in [dataconnect-generated/index.d.ts](./index.d.ts). It has the following fields:
```typescript
export interface GetCelestialBodyData {
  celestialBody?: {
    commonName: string;
    scientificName: string;
  };
}
```
### Using `GetCelestialBody`'s action shortcut function

```typescript
import { getDataConnect } from 'firebase/data-connect';
import { connectorConfig, getCelestialBody } from '@dataconnect/generated';


// Call the `getCelestialBody()` function to execute the query.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await getCelestialBody();

// You can also pass in a `DataConnect` instance to the action shortcut function.
const dataConnect = getDataConnect(connectorConfig);
const { data } = await getCelestialBody(dataConnect);

console.log(data.celestialBody);

// Or, you can use the `Promise` API.
getCelestialBody().then((response) => {
  const data = response.data;
  console.log(data.celestialBody);
});
```

### Using `GetCelestialBody`'s `QueryRef` function

```typescript
import { getDataConnect, executeQuery } from 'firebase/data-connect';
import { connectorConfig, getCelestialBodyRef } from '@dataconnect/generated';


// Call the `getCelestialBodyRef()` function to get a reference to the query.
const ref = getCelestialBodyRef();

// You can also pass in a `DataConnect` instance to the `QueryRef` function.
const dataConnect = getDataConnect(connectorConfig);
const ref = getCelestialBodyRef(dataConnect);

// Call `executeQuery()` on the reference to execute the query.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await executeQuery(ref);

console.log(data.celestialBody);

// Or, you can use the `Promise` API.
executeQuery(ref).then((response) => {
  const data = response.data;
  console.log(data.celestialBody);
});
```

## ListCelestialBodies
You can execute the `ListCelestialBodies` query using the following action shortcut function, or by calling `executeQuery()` after calling the following `QueryRef` function, both of which are defined in [dataconnect-generated/index.d.ts](./index.d.ts):
```typescript
listCelestialBodies(options?: ExecuteQueryOptions): QueryPromise<ListCelestialBodiesData, undefined>;

interface ListCelestialBodiesRef {
  ...
  /* Allow users to create refs without passing in DataConnect */
  (): QueryRef<ListCelestialBodiesData, undefined>;
}
export const listCelestialBodiesRef: ListCelestialBodiesRef;
```
You can also pass in a `DataConnect` instance to the action shortcut function or `QueryRef` function.
```typescript
listCelestialBodies(dc: DataConnect, options?: ExecuteQueryOptions): QueryPromise<ListCelestialBodiesData, undefined>;

interface ListCelestialBodiesRef {
  ...
  (dc: DataConnect): QueryRef<ListCelestialBodiesData, undefined>;
}
export const listCelestialBodiesRef: ListCelestialBodiesRef;
```

If you need the name of the operation without creating a ref, you can retrieve the operation name by calling the `operationName` property on the listCelestialBodiesRef:
```typescript
const name = listCelestialBodiesRef.operationName;
console.log(name);
```

### Variables
The `ListCelestialBodies` query has no variables.
### Return Type
Recall that executing the `ListCelestialBodies` query returns a `QueryPromise` that resolves to an object with a `data` property.

The `data` property is an object of type `ListCelestialBodiesData`, which is defined in [dataconnect-generated/index.d.ts](./index.d.ts). It has the following fields:
```typescript
export interface ListCelestialBodiesData {
  celestialBodies: ({
    commonName: string;
    objectType: string;
  })[];
}
```
### Using `ListCelestialBodies`'s action shortcut function

```typescript
import { getDataConnect } from 'firebase/data-connect';
import { connectorConfig, listCelestialBodies } from '@dataconnect/generated';


// Call the `listCelestialBodies()` function to execute the query.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await listCelestialBodies();

// You can also pass in a `DataConnect` instance to the action shortcut function.
const dataConnect = getDataConnect(connectorConfig);
const { data } = await listCelestialBodies(dataConnect);

console.log(data.celestialBodies);

// Or, you can use the `Promise` API.
listCelestialBodies().then((response) => {
  const data = response.data;
  console.log(data.celestialBodies);
});
```

### Using `ListCelestialBodies`'s `QueryRef` function

```typescript
import { getDataConnect, executeQuery } from 'firebase/data-connect';
import { connectorConfig, listCelestialBodiesRef } from '@dataconnect/generated';


// Call the `listCelestialBodiesRef()` function to get a reference to the query.
const ref = listCelestialBodiesRef();

// You can also pass in a `DataConnect` instance to the `QueryRef` function.
const dataConnect = getDataConnect(connectorConfig);
const ref = listCelestialBodiesRef(dataConnect);

// Call `executeQuery()` on the reference to execute the query.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await executeQuery(ref);

console.log(data.celestialBodies);

// Or, you can use the `Promise` API.
executeQuery(ref).then((response) => {
  const data = response.data;
  console.log(data.celestialBodies);
});
```

## GetEvent
You can execute the `GetEvent` query using the following action shortcut function, or by calling `executeQuery()` after calling the following `QueryRef` function, both of which are defined in [dataconnect-generated/index.d.ts](./index.d.ts):
```typescript
getEvent(options?: ExecuteQueryOptions): QueryPromise<GetEventData, undefined>;

interface GetEventRef {
  ...
  /* Allow users to create refs without passing in DataConnect */
  (): QueryRef<GetEventData, undefined>;
}
export const getEventRef: GetEventRef;
```
You can also pass in a `DataConnect` instance to the action shortcut function or `QueryRef` function.
```typescript
getEvent(dc: DataConnect, options?: ExecuteQueryOptions): QueryPromise<GetEventData, undefined>;

interface GetEventRef {
  ...
  (dc: DataConnect): QueryRef<GetEventData, undefined>;
}
export const getEventRef: GetEventRef;
```

If you need the name of the operation without creating a ref, you can retrieve the operation name by calling the `operationName` property on the getEventRef:
```typescript
const name = getEventRef.operationName;
console.log(name);
```

### Variables
The `GetEvent` query has no variables.
### Return Type
Recall that executing the `GetEvent` query returns a `QueryPromise` that resolves to an object with a `data` property.

The `data` property is an object of type `GetEventData`, which is defined in [dataconnect-generated/index.d.ts](./index.d.ts). It has the following fields:
```typescript
export interface GetEventData {
  event?: {
    eventName: string;
    description?: string | null;
  };
}
```
### Using `GetEvent`'s action shortcut function

```typescript
import { getDataConnect } from 'firebase/data-connect';
import { connectorConfig, getEvent } from '@dataconnect/generated';


// Call the `getEvent()` function to execute the query.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await getEvent();

// You can also pass in a `DataConnect` instance to the action shortcut function.
const dataConnect = getDataConnect(connectorConfig);
const { data } = await getEvent(dataConnect);

console.log(data.event);

// Or, you can use the `Promise` API.
getEvent().then((response) => {
  const data = response.data;
  console.log(data.event);
});
```

### Using `GetEvent`'s `QueryRef` function

```typescript
import { getDataConnect, executeQuery } from 'firebase/data-connect';
import { connectorConfig, getEventRef } from '@dataconnect/generated';


// Call the `getEventRef()` function to get a reference to the query.
const ref = getEventRef();

// You can also pass in a `DataConnect` instance to the `QueryRef` function.
const dataConnect = getDataConnect(connectorConfig);
const ref = getEventRef(dataConnect);

// Call `executeQuery()` on the reference to execute the query.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await executeQuery(ref);

console.log(data.event);

// Or, you can use the `Promise` API.
executeQuery(ref).then((response) => {
  const data = response.data;
  console.log(data.event);
});
```

## ListEvents
You can execute the `ListEvents` query using the following action shortcut function, or by calling `executeQuery()` after calling the following `QueryRef` function, both of which are defined in [dataconnect-generated/index.d.ts](./index.d.ts):
```typescript
listEvents(options?: ExecuteQueryOptions): QueryPromise<ListEventsData, undefined>;

interface ListEventsRef {
  ...
  /* Allow users to create refs without passing in DataConnect */
  (): QueryRef<ListEventsData, undefined>;
}
export const listEventsRef: ListEventsRef;
```
You can also pass in a `DataConnect` instance to the action shortcut function or `QueryRef` function.
```typescript
listEvents(dc: DataConnect, options?: ExecuteQueryOptions): QueryPromise<ListEventsData, undefined>;

interface ListEventsRef {
  ...
  (dc: DataConnect): QueryRef<ListEventsData, undefined>;
}
export const listEventsRef: ListEventsRef;
```

If you need the name of the operation without creating a ref, you can retrieve the operation name by calling the `operationName` property on the listEventsRef:
```typescript
const name = listEventsRef.operationName;
console.log(name);
```

### Variables
The `ListEvents` query has no variables.
### Return Type
Recall that executing the `ListEvents` query returns a `QueryPromise` that resolves to an object with a `data` property.

The `data` property is an object of type `ListEventsData`, which is defined in [dataconnect-generated/index.d.ts](./index.d.ts). It has the following fields:
```typescript
export interface ListEventsData {
  events: ({
    eventName: string;
    startTime: TimestampString;
  })[];
}
```
### Using `ListEvents`'s action shortcut function

```typescript
import { getDataConnect } from 'firebase/data-connect';
import { connectorConfig, listEvents } from '@dataconnect/generated';


// Call the `listEvents()` function to execute the query.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await listEvents();

// You can also pass in a `DataConnect` instance to the action shortcut function.
const dataConnect = getDataConnect(connectorConfig);
const { data } = await listEvents(dataConnect);

console.log(data.events);

// Or, you can use the `Promise` API.
listEvents().then((response) => {
  const data = response.data;
  console.log(data.events);
});
```

### Using `ListEvents`'s `QueryRef` function

```typescript
import { getDataConnect, executeQuery } from 'firebase/data-connect';
import { connectorConfig, listEventsRef } from '@dataconnect/generated';


// Call the `listEventsRef()` function to get a reference to the query.
const ref = listEventsRef();

// You can also pass in a `DataConnect` instance to the `QueryRef` function.
const dataConnect = getDataConnect(connectorConfig);
const ref = listEventsRef(dataConnect);

// Call `executeQuery()` on the reference to execute the query.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await executeQuery(ref);

console.log(data.events);

// Or, you can use the `Promise` API.
executeQuery(ref).then((response) => {
  const data = response.data;
  console.log(data.events);
});
```

## GetSession
You can execute the `GetSession` query using the following action shortcut function, or by calling `executeQuery()` after calling the following `QueryRef` function, both of which are defined in [dataconnect-generated/index.d.ts](./index.d.ts):
```typescript
getSession(options?: ExecuteQueryOptions): QueryPromise<GetSessionData, undefined>;

interface GetSessionRef {
  ...
  /* Allow users to create refs without passing in DataConnect */
  (): QueryRef<GetSessionData, undefined>;
}
export const getSessionRef: GetSessionRef;
```
You can also pass in a `DataConnect` instance to the action shortcut function or `QueryRef` function.
```typescript
getSession(dc: DataConnect, options?: ExecuteQueryOptions): QueryPromise<GetSessionData, undefined>;

interface GetSessionRef {
  ...
  (dc: DataConnect): QueryRef<GetSessionData, undefined>;
}
export const getSessionRef: GetSessionRef;
```

If you need the name of the operation without creating a ref, you can retrieve the operation name by calling the `operationName` property on the getSessionRef:
```typescript
const name = getSessionRef.operationName;
console.log(name);
```

### Variables
The `GetSession` query has no variables.
### Return Type
Recall that executing the `GetSession` query returns a `QueryPromise` that resolves to an object with a `data` property.

The `data` property is an object of type `GetSessionData`, which is defined in [dataconnect-generated/index.d.ts](./index.d.ts). It has the following fields:
```typescript
export interface GetSessionData {
  session?: {
    sessionName: string;
    user: {
      email: string;
    };
  };
}
```
### Using `GetSession`'s action shortcut function

```typescript
import { getDataConnect } from 'firebase/data-connect';
import { connectorConfig, getSession } from '@dataconnect/generated';


// Call the `getSession()` function to execute the query.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await getSession();

// You can also pass in a `DataConnect` instance to the action shortcut function.
const dataConnect = getDataConnect(connectorConfig);
const { data } = await getSession(dataConnect);

console.log(data.session);

// Or, you can use the `Promise` API.
getSession().then((response) => {
  const data = response.data;
  console.log(data.session);
});
```

### Using `GetSession`'s `QueryRef` function

```typescript
import { getDataConnect, executeQuery } from 'firebase/data-connect';
import { connectorConfig, getSessionRef } from '@dataconnect/generated';


// Call the `getSessionRef()` function to get a reference to the query.
const ref = getSessionRef();

// You can also pass in a `DataConnect` instance to the `QueryRef` function.
const dataConnect = getDataConnect(connectorConfig);
const ref = getSessionRef(dataConnect);

// Call `executeQuery()` on the reference to execute the query.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await executeQuery(ref);

console.log(data.session);

// Or, you can use the `Promise` API.
executeQuery(ref).then((response) => {
  const data = response.data;
  console.log(data.session);
});
```

## ListMySessions
You can execute the `ListMySessions` query using the following action shortcut function, or by calling `executeQuery()` after calling the following `QueryRef` function, both of which are defined in [dataconnect-generated/index.d.ts](./index.d.ts):
```typescript
listMySessions(options?: ExecuteQueryOptions): QueryPromise<ListMySessionsData, undefined>;

interface ListMySessionsRef {
  ...
  /* Allow users to create refs without passing in DataConnect */
  (): QueryRef<ListMySessionsData, undefined>;
}
export const listMySessionsRef: ListMySessionsRef;
```
You can also pass in a `DataConnect` instance to the action shortcut function or `QueryRef` function.
```typescript
listMySessions(dc: DataConnect, options?: ExecuteQueryOptions): QueryPromise<ListMySessionsData, undefined>;

interface ListMySessionsRef {
  ...
  (dc: DataConnect): QueryRef<ListMySessionsData, undefined>;
}
export const listMySessionsRef: ListMySessionsRef;
```

If you need the name of the operation without creating a ref, you can retrieve the operation name by calling the `operationName` property on the listMySessionsRef:
```typescript
const name = listMySessionsRef.operationName;
console.log(name);
```

### Variables
The `ListMySessions` query has no variables.
### Return Type
Recall that executing the `ListMySessions` query returns a `QueryPromise` that resolves to an object with a `data` property.

The `data` property is an object of type `ListMySessionsData`, which is defined in [dataconnect-generated/index.d.ts](./index.d.ts). It has the following fields:
```typescript
export interface ListMySessionsData {
  sessions: ({
    sessionName: string;
    startDate?: DateString | null;
  })[];
}
```
### Using `ListMySessions`'s action shortcut function

```typescript
import { getDataConnect } from 'firebase/data-connect';
import { connectorConfig, listMySessions } from '@dataconnect/generated';


// Call the `listMySessions()` function to execute the query.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await listMySessions();

// You can also pass in a `DataConnect` instance to the action shortcut function.
const dataConnect = getDataConnect(connectorConfig);
const { data } = await listMySessions(dataConnect);

console.log(data.sessions);

// Or, you can use the `Promise` API.
listMySessions().then((response) => {
  const data = response.data;
  console.log(data.sessions);
});
```

### Using `ListMySessions`'s `QueryRef` function

```typescript
import { getDataConnect, executeQuery } from 'firebase/data-connect';
import { connectorConfig, listMySessionsRef } from '@dataconnect/generated';


// Call the `listMySessionsRef()` function to get a reference to the query.
const ref = listMySessionsRef();

// You can also pass in a `DataConnect` instance to the `QueryRef` function.
const dataConnect = getDataConnect(connectorConfig);
const ref = listMySessionsRef(dataConnect);

// Call `executeQuery()` on the reference to execute the query.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await executeQuery(ref);

console.log(data.sessions);

// Or, you can use the `Promise` API.
executeQuery(ref).then((response) => {
  const data = response.data;
  console.log(data.sessions);
});
```

## GetObservation
You can execute the `GetObservation` query using the following action shortcut function, or by calling `executeQuery()` after calling the following `QueryRef` function, both of which are defined in [dataconnect-generated/index.d.ts](./index.d.ts):
```typescript
getObservation(options?: ExecuteQueryOptions): QueryPromise<GetObservationData, undefined>;

interface GetObservationRef {
  ...
  /* Allow users to create refs without passing in DataConnect */
  (): QueryRef<GetObservationData, undefined>;
}
export const getObservationRef: GetObservationRef;
```
You can also pass in a `DataConnect` instance to the action shortcut function or `QueryRef` function.
```typescript
getObservation(dc: DataConnect, options?: ExecuteQueryOptions): QueryPromise<GetObservationData, undefined>;

interface GetObservationRef {
  ...
  (dc: DataConnect): QueryRef<GetObservationData, undefined>;
}
export const getObservationRef: GetObservationRef;
```

If you need the name of the operation without creating a ref, you can retrieve the operation name by calling the `operationName` property on the getObservationRef:
```typescript
const name = getObservationRef.operationName;
console.log(name);
```

### Variables
The `GetObservation` query has no variables.
### Return Type
Recall that executing the `GetObservation` query returns a `QueryPromise` that resolves to an object with a `data` property.

The `data` property is an object of type `GetObservationData`, which is defined in [dataconnect-generated/index.d.ts](./index.d.ts). It has the following fields:
```typescript
export interface GetObservationData {
  observation?: {
    notes?: string | null;
    celestialBody: {
      commonName: string;
    };
  };
}
```
### Using `GetObservation`'s action shortcut function

```typescript
import { getDataConnect } from 'firebase/data-connect';
import { connectorConfig, getObservation } from '@dataconnect/generated';


// Call the `getObservation()` function to execute the query.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await getObservation();

// You can also pass in a `DataConnect` instance to the action shortcut function.
const dataConnect = getDataConnect(connectorConfig);
const { data } = await getObservation(dataConnect);

console.log(data.observation);

// Or, you can use the `Promise` API.
getObservation().then((response) => {
  const data = response.data;
  console.log(data.observation);
});
```

### Using `GetObservation`'s `QueryRef` function

```typescript
import { getDataConnect, executeQuery } from 'firebase/data-connect';
import { connectorConfig, getObservationRef } from '@dataconnect/generated';


// Call the `getObservationRef()` function to get a reference to the query.
const ref = getObservationRef();

// You can also pass in a `DataConnect` instance to the `QueryRef` function.
const dataConnect = getDataConnect(connectorConfig);
const ref = getObservationRef(dataConnect);

// Call `executeQuery()` on the reference to execute the query.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await executeQuery(ref);

console.log(data.observation);

// Or, you can use the `Promise` API.
executeQuery(ref).then((response) => {
  const data = response.data;
  console.log(data.observation);
});
```

## ListMyObservations
You can execute the `ListMyObservations` query using the following action shortcut function, or by calling `executeQuery()` after calling the following `QueryRef` function, both of which are defined in [dataconnect-generated/index.d.ts](./index.d.ts):
```typescript
listMyObservations(options?: ExecuteQueryOptions): QueryPromise<ListMyObservationsData, undefined>;

interface ListMyObservationsRef {
  ...
  /* Allow users to create refs without passing in DataConnect */
  (): QueryRef<ListMyObservationsData, undefined>;
}
export const listMyObservationsRef: ListMyObservationsRef;
```
You can also pass in a `DataConnect` instance to the action shortcut function or `QueryRef` function.
```typescript
listMyObservations(dc: DataConnect, options?: ExecuteQueryOptions): QueryPromise<ListMyObservationsData, undefined>;

interface ListMyObservationsRef {
  ...
  (dc: DataConnect): QueryRef<ListMyObservationsData, undefined>;
}
export const listMyObservationsRef: ListMyObservationsRef;
```

If you need the name of the operation without creating a ref, you can retrieve the operation name by calling the `operationName` property on the listMyObservationsRef:
```typescript
const name = listMyObservationsRef.operationName;
console.log(name);
```

### Variables
The `ListMyObservations` query has no variables.
### Return Type
Recall that executing the `ListMyObservations` query returns a `QueryPromise` that resolves to an object with a `data` property.

The `data` property is an object of type `ListMyObservationsData`, which is defined in [dataconnect-generated/index.d.ts](./index.d.ts). It has the following fields:
```typescript
export interface ListMyObservationsData {
  observations: ({
    observationTimestamp: TimestampString;
    notes?: string | null;
  })[];
}
```
### Using `ListMyObservations`'s action shortcut function

```typescript
import { getDataConnect } from 'firebase/data-connect';
import { connectorConfig, listMyObservations } from '@dataconnect/generated';


// Call the `listMyObservations()` function to execute the query.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await listMyObservations();

// You can also pass in a `DataConnect` instance to the action shortcut function.
const dataConnect = getDataConnect(connectorConfig);
const { data } = await listMyObservations(dataConnect);

console.log(data.observations);

// Or, you can use the `Promise` API.
listMyObservations().then((response) => {
  const data = response.data;
  console.log(data.observations);
});
```

### Using `ListMyObservations`'s `QueryRef` function

```typescript
import { getDataConnect, executeQuery } from 'firebase/data-connect';
import { connectorConfig, listMyObservationsRef } from '@dataconnect/generated';


// Call the `listMyObservationsRef()` function to get a reference to the query.
const ref = listMyObservationsRef();

// You can also pass in a `DataConnect` instance to the `QueryRef` function.
const dataConnect = getDataConnect(connectorConfig);
const ref = listMyObservationsRef(dataConnect);

// Call `executeQuery()` on the reference to execute the query.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await executeQuery(ref);

console.log(data.observations);

// Or, you can use the `Promise` API.
executeQuery(ref).then((response) => {
  const data = response.data;
  console.log(data.observations);
});
```

## GetPhotoSetting
You can execute the `GetPhotoSetting` query using the following action shortcut function, or by calling `executeQuery()` after calling the following `QueryRef` function, both of which are defined in [dataconnect-generated/index.d.ts](./index.d.ts):
```typescript
getPhotoSetting(options?: ExecuteQueryOptions): QueryPromise<GetPhotoSettingData, undefined>;

interface GetPhotoSettingRef {
  ...
  /* Allow users to create refs without passing in DataConnect */
  (): QueryRef<GetPhotoSettingData, undefined>;
}
export const getPhotoSettingRef: GetPhotoSettingRef;
```
You can also pass in a `DataConnect` instance to the action shortcut function or `QueryRef` function.
```typescript
getPhotoSetting(dc: DataConnect, options?: ExecuteQueryOptions): QueryPromise<GetPhotoSettingData, undefined>;

interface GetPhotoSettingRef {
  ...
  (dc: DataConnect): QueryRef<GetPhotoSettingData, undefined>;
}
export const getPhotoSettingRef: GetPhotoSettingRef;
```

If you need the name of the operation without creating a ref, you can retrieve the operation name by calling the `operationName` property on the getPhotoSettingRef:
```typescript
const name = getPhotoSettingRef.operationName;
console.log(name);
```

### Variables
The `GetPhotoSetting` query has no variables.
### Return Type
Recall that executing the `GetPhotoSetting` query returns a `QueryPromise` that resolves to an object with a `data` property.

The `data` property is an object of type `GetPhotoSettingData`, which is defined in [dataconnect-generated/index.d.ts](./index.d.ts). It has the following fields:
```typescript
export interface GetPhotoSettingData {
  photoSetting?: {
    cameraModel: string;
    iso: number;
  };
}
```
### Using `GetPhotoSetting`'s action shortcut function

```typescript
import { getDataConnect } from 'firebase/data-connect';
import { connectorConfig, getPhotoSetting } from '@dataconnect/generated';


// Call the `getPhotoSetting()` function to execute the query.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await getPhotoSetting();

// You can also pass in a `DataConnect` instance to the action shortcut function.
const dataConnect = getDataConnect(connectorConfig);
const { data } = await getPhotoSetting(dataConnect);

console.log(data.photoSetting);

// Or, you can use the `Promise` API.
getPhotoSetting().then((response) => {
  const data = response.data;
  console.log(data.photoSetting);
});
```

### Using `GetPhotoSetting`'s `QueryRef` function

```typescript
import { getDataConnect, executeQuery } from 'firebase/data-connect';
import { connectorConfig, getPhotoSettingRef } from '@dataconnect/generated';


// Call the `getPhotoSettingRef()` function to get a reference to the query.
const ref = getPhotoSettingRef();

// You can also pass in a `DataConnect` instance to the `QueryRef` function.
const dataConnect = getDataConnect(connectorConfig);
const ref = getPhotoSettingRef(dataConnect);

// Call `executeQuery()` on the reference to execute the query.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await executeQuery(ref);

console.log(data.photoSetting);

// Or, you can use the `Promise` API.
executeQuery(ref).then((response) => {
  const data = response.data;
  console.log(data.photoSetting);
});
```

## ListMyPhotoSettings
You can execute the `ListMyPhotoSettings` query using the following action shortcut function, or by calling `executeQuery()` after calling the following `QueryRef` function, both of which are defined in [dataconnect-generated/index.d.ts](./index.d.ts):
```typescript
listMyPhotoSettings(options?: ExecuteQueryOptions): QueryPromise<ListMyPhotoSettingsData, undefined>;

interface ListMyPhotoSettingsRef {
  ...
  /* Allow users to create refs without passing in DataConnect */
  (): QueryRef<ListMyPhotoSettingsData, undefined>;
}
export const listMyPhotoSettingsRef: ListMyPhotoSettingsRef;
```
You can also pass in a `DataConnect` instance to the action shortcut function or `QueryRef` function.
```typescript
listMyPhotoSettings(dc: DataConnect, options?: ExecuteQueryOptions): QueryPromise<ListMyPhotoSettingsData, undefined>;

interface ListMyPhotoSettingsRef {
  ...
  (dc: DataConnect): QueryRef<ListMyPhotoSettingsData, undefined>;
}
export const listMyPhotoSettingsRef: ListMyPhotoSettingsRef;
```

If you need the name of the operation without creating a ref, you can retrieve the operation name by calling the `operationName` property on the listMyPhotoSettingsRef:
```typescript
const name = listMyPhotoSettingsRef.operationName;
console.log(name);
```

### Variables
The `ListMyPhotoSettings` query has no variables.
### Return Type
Recall that executing the `ListMyPhotoSettings` query returns a `QueryPromise` that resolves to an object with a `data` property.

The `data` property is an object of type `ListMyPhotoSettingsData`, which is defined in [dataconnect-generated/index.d.ts](./index.d.ts). It has the following fields:
```typescript
export interface ListMyPhotoSettingsData {
  photoSettings: ({
    cameraModel: string;
    exposureSeconds: number;
  })[];
}
```
### Using `ListMyPhotoSettings`'s action shortcut function

```typescript
import { getDataConnect } from 'firebase/data-connect';
import { connectorConfig, listMyPhotoSettings } from '@dataconnect/generated';


// Call the `listMyPhotoSettings()` function to execute the query.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await listMyPhotoSettings();

// You can also pass in a `DataConnect` instance to the action shortcut function.
const dataConnect = getDataConnect(connectorConfig);
const { data } = await listMyPhotoSettings(dataConnect);

console.log(data.photoSettings);

// Or, you can use the `Promise` API.
listMyPhotoSettings().then((response) => {
  const data = response.data;
  console.log(data.photoSettings);
});
```

### Using `ListMyPhotoSettings`'s `QueryRef` function

```typescript
import { getDataConnect, executeQuery } from 'firebase/data-connect';
import { connectorConfig, listMyPhotoSettingsRef } from '@dataconnect/generated';


// Call the `listMyPhotoSettingsRef()` function to get a reference to the query.
const ref = listMyPhotoSettingsRef();

// You can also pass in a `DataConnect` instance to the `QueryRef` function.
const dataConnect = getDataConnect(connectorConfig);
const ref = listMyPhotoSettingsRef(dataConnect);

// Call `executeQuery()` on the reference to execute the query.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await executeQuery(ref);

console.log(data.photoSettings);

// Or, you can use the `Promise` API.
executeQuery(ref).then((response) => {
  const data = response.data;
  console.log(data.photoSettings);
});
```

# Mutations

There are two ways to execute a Data Connect Mutation using the generated Web SDK:
- Using a Mutation Reference function, which returns a `MutationRef`
  - The `MutationRef` can be used as an argument to `executeMutation()`, which will execute the Mutation and return a `MutationPromise`
- Using an action shortcut function, which returns a `MutationPromise`
  - Calling the action shortcut function will execute the Mutation and return a `MutationPromise`

The following is true for both the action shortcut function and the `MutationRef` function:
- The `MutationPromise` returned will resolve to the result of the Mutation once it has finished executing
- If the Mutation accepts arguments, both the action shortcut function and the `MutationRef` function accept a single argument: an object that contains all the required variables (and the optional variables) for the Mutation
- Both functions can be called with or without passing in a `DataConnect` instance as an argument. If no `DataConnect` argument is passed in, then the generated SDK will call `getDataConnect(connectorConfig)` behind the scenes for you.

Below are examples of how to use the `example` connector's generated functions to execute each mutation. You can also follow the examples from the [Data Connect documentation](https://firebase.google.com/docs/data-connect/web-sdk#using-mutations).

## CreateUser
You can execute the `CreateUser` mutation using the following action shortcut function, or by calling `executeMutation()` after calling the following `MutationRef` function, both of which are defined in [dataconnect-generated/index.d.ts](./index.d.ts):
```typescript
createUser(): MutationPromise<CreateUserData, undefined>;

interface CreateUserRef {
  ...
  /* Allow users to create refs without passing in DataConnect */
  (): MutationRef<CreateUserData, undefined>;
}
export const createUserRef: CreateUserRef;
```
You can also pass in a `DataConnect` instance to the action shortcut function or `MutationRef` function.
```typescript
createUser(dc: DataConnect): MutationPromise<CreateUserData, undefined>;

interface CreateUserRef {
  ...
  (dc: DataConnect): MutationRef<CreateUserData, undefined>;
}
export const createUserRef: CreateUserRef;
```

If you need the name of the operation without creating a ref, you can retrieve the operation name by calling the `operationName` property on the createUserRef:
```typescript
const name = createUserRef.operationName;
console.log(name);
```

### Variables
The `CreateUser` mutation has no variables.
### Return Type
Recall that executing the `CreateUser` mutation returns a `MutationPromise` that resolves to an object with a `data` property.

The `data` property is an object of type `CreateUserData`, which is defined in [dataconnect-generated/index.d.ts](./index.d.ts). It has the following fields:
```typescript
export interface CreateUserData {
  user_insert: User_Key;
}
```
### Using `CreateUser`'s action shortcut function

```typescript
import { getDataConnect } from 'firebase/data-connect';
import { connectorConfig, createUser } from '@dataconnect/generated';


// Call the `createUser()` function to execute the mutation.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await createUser();

// You can also pass in a `DataConnect` instance to the action shortcut function.
const dataConnect = getDataConnect(connectorConfig);
const { data } = await createUser(dataConnect);

console.log(data.user_insert);

// Or, you can use the `Promise` API.
createUser().then((response) => {
  const data = response.data;
  console.log(data.user_insert);
});
```

### Using `CreateUser`'s `MutationRef` function

```typescript
import { getDataConnect, executeMutation } from 'firebase/data-connect';
import { connectorConfig, createUserRef } from '@dataconnect/generated';


// Call the `createUserRef()` function to get a reference to the mutation.
const ref = createUserRef();

// You can also pass in a `DataConnect` instance to the `MutationRef` function.
const dataConnect = getDataConnect(connectorConfig);
const ref = createUserRef(dataConnect);

// Call `executeMutation()` on the reference to execute the mutation.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await executeMutation(ref);

console.log(data.user_insert);

// Or, you can use the `Promise` API.
executeMutation(ref).then((response) => {
  const data = response.data;
  console.log(data.user_insert);
});
```

## UpdateUser
You can execute the `UpdateUser` mutation using the following action shortcut function, or by calling `executeMutation()` after calling the following `MutationRef` function, both of which are defined in [dataconnect-generated/index.d.ts](./index.d.ts):
```typescript
updateUser(): MutationPromise<UpdateUserData, undefined>;

interface UpdateUserRef {
  ...
  /* Allow users to create refs without passing in DataConnect */
  (): MutationRef<UpdateUserData, undefined>;
}
export const updateUserRef: UpdateUserRef;
```
You can also pass in a `DataConnect` instance to the action shortcut function or `MutationRef` function.
```typescript
updateUser(dc: DataConnect): MutationPromise<UpdateUserData, undefined>;

interface UpdateUserRef {
  ...
  (dc: DataConnect): MutationRef<UpdateUserData, undefined>;
}
export const updateUserRef: UpdateUserRef;
```

If you need the name of the operation without creating a ref, you can retrieve the operation name by calling the `operationName` property on the updateUserRef:
```typescript
const name = updateUserRef.operationName;
console.log(name);
```

### Variables
The `UpdateUser` mutation has no variables.
### Return Type
Recall that executing the `UpdateUser` mutation returns a `MutationPromise` that resolves to an object with a `data` property.

The `data` property is an object of type `UpdateUserData`, which is defined in [dataconnect-generated/index.d.ts](./index.d.ts). It has the following fields:
```typescript
export interface UpdateUserData {
  user_update?: User_Key | null;
}
```
### Using `UpdateUser`'s action shortcut function

```typescript
import { getDataConnect } from 'firebase/data-connect';
import { connectorConfig, updateUser } from '@dataconnect/generated';


// Call the `updateUser()` function to execute the mutation.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await updateUser();

// You can also pass in a `DataConnect` instance to the action shortcut function.
const dataConnect = getDataConnect(connectorConfig);
const { data } = await updateUser(dataConnect);

console.log(data.user_update);

// Or, you can use the `Promise` API.
updateUser().then((response) => {
  const data = response.data;
  console.log(data.user_update);
});
```

### Using `UpdateUser`'s `MutationRef` function

```typescript
import { getDataConnect, executeMutation } from 'firebase/data-connect';
import { connectorConfig, updateUserRef } from '@dataconnect/generated';


// Call the `updateUserRef()` function to get a reference to the mutation.
const ref = updateUserRef();

// You can also pass in a `DataConnect` instance to the `MutationRef` function.
const dataConnect = getDataConnect(connectorConfig);
const ref = updateUserRef(dataConnect);

// Call `executeMutation()` on the reference to execute the mutation.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await executeMutation(ref);

console.log(data.user_update);

// Or, you can use the `Promise` API.
executeMutation(ref).then((response) => {
  const data = response.data;
  console.log(data.user_update);
});
```

## DeleteUser
You can execute the `DeleteUser` mutation using the following action shortcut function, or by calling `executeMutation()` after calling the following `MutationRef` function, both of which are defined in [dataconnect-generated/index.d.ts](./index.d.ts):
```typescript
deleteUser(): MutationPromise<DeleteUserData, undefined>;

interface DeleteUserRef {
  ...
  /* Allow users to create refs without passing in DataConnect */
  (): MutationRef<DeleteUserData, undefined>;
}
export const deleteUserRef: DeleteUserRef;
```
You can also pass in a `DataConnect` instance to the action shortcut function or `MutationRef` function.
```typescript
deleteUser(dc: DataConnect): MutationPromise<DeleteUserData, undefined>;

interface DeleteUserRef {
  ...
  (dc: DataConnect): MutationRef<DeleteUserData, undefined>;
}
export const deleteUserRef: DeleteUserRef;
```

If you need the name of the operation without creating a ref, you can retrieve the operation name by calling the `operationName` property on the deleteUserRef:
```typescript
const name = deleteUserRef.operationName;
console.log(name);
```

### Variables
The `DeleteUser` mutation has no variables.
### Return Type
Recall that executing the `DeleteUser` mutation returns a `MutationPromise` that resolves to an object with a `data` property.

The `data` property is an object of type `DeleteUserData`, which is defined in [dataconnect-generated/index.d.ts](./index.d.ts). It has the following fields:
```typescript
export interface DeleteUserData {
  user_delete?: User_Key | null;
}
```
### Using `DeleteUser`'s action shortcut function

```typescript
import { getDataConnect } from 'firebase/data-connect';
import { connectorConfig, deleteUser } from '@dataconnect/generated';


// Call the `deleteUser()` function to execute the mutation.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await deleteUser();

// You can also pass in a `DataConnect` instance to the action shortcut function.
const dataConnect = getDataConnect(connectorConfig);
const { data } = await deleteUser(dataConnect);

console.log(data.user_delete);

// Or, you can use the `Promise` API.
deleteUser().then((response) => {
  const data = response.data;
  console.log(data.user_delete);
});
```

### Using `DeleteUser`'s `MutationRef` function

```typescript
import { getDataConnect, executeMutation } from 'firebase/data-connect';
import { connectorConfig, deleteUserRef } from '@dataconnect/generated';


// Call the `deleteUserRef()` function to get a reference to the mutation.
const ref = deleteUserRef();

// You can also pass in a `DataConnect` instance to the `MutationRef` function.
const dataConnect = getDataConnect(connectorConfig);
const ref = deleteUserRef(dataConnect);

// Call `executeMutation()` on the reference to execute the mutation.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await executeMutation(ref);

console.log(data.user_delete);

// Or, you can use the `Promise` API.
executeMutation(ref).then((response) => {
  const data = response.data;
  console.log(data.user_delete);
});
```

## CreateCelestialBody
You can execute the `CreateCelestialBody` mutation using the following action shortcut function, or by calling `executeMutation()` after calling the following `MutationRef` function, both of which are defined in [dataconnect-generated/index.d.ts](./index.d.ts):
```typescript
createCelestialBody(): MutationPromise<CreateCelestialBodyData, undefined>;

interface CreateCelestialBodyRef {
  ...
  /* Allow users to create refs without passing in DataConnect */
  (): MutationRef<CreateCelestialBodyData, undefined>;
}
export const createCelestialBodyRef: CreateCelestialBodyRef;
```
You can also pass in a `DataConnect` instance to the action shortcut function or `MutationRef` function.
```typescript
createCelestialBody(dc: DataConnect): MutationPromise<CreateCelestialBodyData, undefined>;

interface CreateCelestialBodyRef {
  ...
  (dc: DataConnect): MutationRef<CreateCelestialBodyData, undefined>;
}
export const createCelestialBodyRef: CreateCelestialBodyRef;
```

If you need the name of the operation without creating a ref, you can retrieve the operation name by calling the `operationName` property on the createCelestialBodyRef:
```typescript
const name = createCelestialBodyRef.operationName;
console.log(name);
```

### Variables
The `CreateCelestialBody` mutation has no variables.
### Return Type
Recall that executing the `CreateCelestialBody` mutation returns a `MutationPromise` that resolves to an object with a `data` property.

The `data` property is an object of type `CreateCelestialBodyData`, which is defined in [dataconnect-generated/index.d.ts](./index.d.ts). It has the following fields:
```typescript
export interface CreateCelestialBodyData {
  celestialBody_insert: CelestialBody_Key;
}
```
### Using `CreateCelestialBody`'s action shortcut function

```typescript
import { getDataConnect } from 'firebase/data-connect';
import { connectorConfig, createCelestialBody } from '@dataconnect/generated';


// Call the `createCelestialBody()` function to execute the mutation.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await createCelestialBody();

// You can also pass in a `DataConnect` instance to the action shortcut function.
const dataConnect = getDataConnect(connectorConfig);
const { data } = await createCelestialBody(dataConnect);

console.log(data.celestialBody_insert);

// Or, you can use the `Promise` API.
createCelestialBody().then((response) => {
  const data = response.data;
  console.log(data.celestialBody_insert);
});
```

### Using `CreateCelestialBody`'s `MutationRef` function

```typescript
import { getDataConnect, executeMutation } from 'firebase/data-connect';
import { connectorConfig, createCelestialBodyRef } from '@dataconnect/generated';


// Call the `createCelestialBodyRef()` function to get a reference to the mutation.
const ref = createCelestialBodyRef();

// You can also pass in a `DataConnect` instance to the `MutationRef` function.
const dataConnect = getDataConnect(connectorConfig);
const ref = createCelestialBodyRef(dataConnect);

// Call `executeMutation()` on the reference to execute the mutation.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await executeMutation(ref);

console.log(data.celestialBody_insert);

// Or, you can use the `Promise` API.
executeMutation(ref).then((response) => {
  const data = response.data;
  console.log(data.celestialBody_insert);
});
```

## UpdateCelestialBody
You can execute the `UpdateCelestialBody` mutation using the following action shortcut function, or by calling `executeMutation()` after calling the following `MutationRef` function, both of which are defined in [dataconnect-generated/index.d.ts](./index.d.ts):
```typescript
updateCelestialBody(): MutationPromise<UpdateCelestialBodyData, undefined>;

interface UpdateCelestialBodyRef {
  ...
  /* Allow users to create refs without passing in DataConnect */
  (): MutationRef<UpdateCelestialBodyData, undefined>;
}
export const updateCelestialBodyRef: UpdateCelestialBodyRef;
```
You can also pass in a `DataConnect` instance to the action shortcut function or `MutationRef` function.
```typescript
updateCelestialBody(dc: DataConnect): MutationPromise<UpdateCelestialBodyData, undefined>;

interface UpdateCelestialBodyRef {
  ...
  (dc: DataConnect): MutationRef<UpdateCelestialBodyData, undefined>;
}
export const updateCelestialBodyRef: UpdateCelestialBodyRef;
```

If you need the name of the operation without creating a ref, you can retrieve the operation name by calling the `operationName` property on the updateCelestialBodyRef:
```typescript
const name = updateCelestialBodyRef.operationName;
console.log(name);
```

### Variables
The `UpdateCelestialBody` mutation has no variables.
### Return Type
Recall that executing the `UpdateCelestialBody` mutation returns a `MutationPromise` that resolves to an object with a `data` property.

The `data` property is an object of type `UpdateCelestialBodyData`, which is defined in [dataconnect-generated/index.d.ts](./index.d.ts). It has the following fields:
```typescript
export interface UpdateCelestialBodyData {
  celestialBody_update?: CelestialBody_Key | null;
}
```
### Using `UpdateCelestialBody`'s action shortcut function

```typescript
import { getDataConnect } from 'firebase/data-connect';
import { connectorConfig, updateCelestialBody } from '@dataconnect/generated';


// Call the `updateCelestialBody()` function to execute the mutation.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await updateCelestialBody();

// You can also pass in a `DataConnect` instance to the action shortcut function.
const dataConnect = getDataConnect(connectorConfig);
const { data } = await updateCelestialBody(dataConnect);

console.log(data.celestialBody_update);

// Or, you can use the `Promise` API.
updateCelestialBody().then((response) => {
  const data = response.data;
  console.log(data.celestialBody_update);
});
```

### Using `UpdateCelestialBody`'s `MutationRef` function

```typescript
import { getDataConnect, executeMutation } from 'firebase/data-connect';
import { connectorConfig, updateCelestialBodyRef } from '@dataconnect/generated';


// Call the `updateCelestialBodyRef()` function to get a reference to the mutation.
const ref = updateCelestialBodyRef();

// You can also pass in a `DataConnect` instance to the `MutationRef` function.
const dataConnect = getDataConnect(connectorConfig);
const ref = updateCelestialBodyRef(dataConnect);

// Call `executeMutation()` on the reference to execute the mutation.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await executeMutation(ref);

console.log(data.celestialBody_update);

// Or, you can use the `Promise` API.
executeMutation(ref).then((response) => {
  const data = response.data;
  console.log(data.celestialBody_update);
});
```

## DeleteCelestialBody
You can execute the `DeleteCelestialBody` mutation using the following action shortcut function, or by calling `executeMutation()` after calling the following `MutationRef` function, both of which are defined in [dataconnect-generated/index.d.ts](./index.d.ts):
```typescript
deleteCelestialBody(): MutationPromise<DeleteCelestialBodyData, undefined>;

interface DeleteCelestialBodyRef {
  ...
  /* Allow users to create refs without passing in DataConnect */
  (): MutationRef<DeleteCelestialBodyData, undefined>;
}
export const deleteCelestialBodyRef: DeleteCelestialBodyRef;
```
You can also pass in a `DataConnect` instance to the action shortcut function or `MutationRef` function.
```typescript
deleteCelestialBody(dc: DataConnect): MutationPromise<DeleteCelestialBodyData, undefined>;

interface DeleteCelestialBodyRef {
  ...
  (dc: DataConnect): MutationRef<DeleteCelestialBodyData, undefined>;
}
export const deleteCelestialBodyRef: DeleteCelestialBodyRef;
```

If you need the name of the operation without creating a ref, you can retrieve the operation name by calling the `operationName` property on the deleteCelestialBodyRef:
```typescript
const name = deleteCelestialBodyRef.operationName;
console.log(name);
```

### Variables
The `DeleteCelestialBody` mutation has no variables.
### Return Type
Recall that executing the `DeleteCelestialBody` mutation returns a `MutationPromise` that resolves to an object with a `data` property.

The `data` property is an object of type `DeleteCelestialBodyData`, which is defined in [dataconnect-generated/index.d.ts](./index.d.ts). It has the following fields:
```typescript
export interface DeleteCelestialBodyData {
  celestialBody_delete?: CelestialBody_Key | null;
}
```
### Using `DeleteCelestialBody`'s action shortcut function

```typescript
import { getDataConnect } from 'firebase/data-connect';
import { connectorConfig, deleteCelestialBody } from '@dataconnect/generated';


// Call the `deleteCelestialBody()` function to execute the mutation.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await deleteCelestialBody();

// You can also pass in a `DataConnect` instance to the action shortcut function.
const dataConnect = getDataConnect(connectorConfig);
const { data } = await deleteCelestialBody(dataConnect);

console.log(data.celestialBody_delete);

// Or, you can use the `Promise` API.
deleteCelestialBody().then((response) => {
  const data = response.data;
  console.log(data.celestialBody_delete);
});
```

### Using `DeleteCelestialBody`'s `MutationRef` function

```typescript
import { getDataConnect, executeMutation } from 'firebase/data-connect';
import { connectorConfig, deleteCelestialBodyRef } from '@dataconnect/generated';


// Call the `deleteCelestialBodyRef()` function to get a reference to the mutation.
const ref = deleteCelestialBodyRef();

// You can also pass in a `DataConnect` instance to the `MutationRef` function.
const dataConnect = getDataConnect(connectorConfig);
const ref = deleteCelestialBodyRef(dataConnect);

// Call `executeMutation()` on the reference to execute the mutation.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await executeMutation(ref);

console.log(data.celestialBody_delete);

// Or, you can use the `Promise` API.
executeMutation(ref).then((response) => {
  const data = response.data;
  console.log(data.celestialBody_delete);
});
```

## CreateEvent
You can execute the `CreateEvent` mutation using the following action shortcut function, or by calling `executeMutation()` after calling the following `MutationRef` function, both of which are defined in [dataconnect-generated/index.d.ts](./index.d.ts):
```typescript
createEvent(): MutationPromise<CreateEventData, undefined>;

interface CreateEventRef {
  ...
  /* Allow users to create refs without passing in DataConnect */
  (): MutationRef<CreateEventData, undefined>;
}
export const createEventRef: CreateEventRef;
```
You can also pass in a `DataConnect` instance to the action shortcut function or `MutationRef` function.
```typescript
createEvent(dc: DataConnect): MutationPromise<CreateEventData, undefined>;

interface CreateEventRef {
  ...
  (dc: DataConnect): MutationRef<CreateEventData, undefined>;
}
export const createEventRef: CreateEventRef;
```

If you need the name of the operation without creating a ref, you can retrieve the operation name by calling the `operationName` property on the createEventRef:
```typescript
const name = createEventRef.operationName;
console.log(name);
```

### Variables
The `CreateEvent` mutation has no variables.
### Return Type
Recall that executing the `CreateEvent` mutation returns a `MutationPromise` that resolves to an object with a `data` property.

The `data` property is an object of type `CreateEventData`, which is defined in [dataconnect-generated/index.d.ts](./index.d.ts). It has the following fields:
```typescript
export interface CreateEventData {
  event_insert: Event_Key;
}
```
### Using `CreateEvent`'s action shortcut function

```typescript
import { getDataConnect } from 'firebase/data-connect';
import { connectorConfig, createEvent } from '@dataconnect/generated';


// Call the `createEvent()` function to execute the mutation.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await createEvent();

// You can also pass in a `DataConnect` instance to the action shortcut function.
const dataConnect = getDataConnect(connectorConfig);
const { data } = await createEvent(dataConnect);

console.log(data.event_insert);

// Or, you can use the `Promise` API.
createEvent().then((response) => {
  const data = response.data;
  console.log(data.event_insert);
});
```

### Using `CreateEvent`'s `MutationRef` function

```typescript
import { getDataConnect, executeMutation } from 'firebase/data-connect';
import { connectorConfig, createEventRef } from '@dataconnect/generated';


// Call the `createEventRef()` function to get a reference to the mutation.
const ref = createEventRef();

// You can also pass in a `DataConnect` instance to the `MutationRef` function.
const dataConnect = getDataConnect(connectorConfig);
const ref = createEventRef(dataConnect);

// Call `executeMutation()` on the reference to execute the mutation.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await executeMutation(ref);

console.log(data.event_insert);

// Or, you can use the `Promise` API.
executeMutation(ref).then((response) => {
  const data = response.data;
  console.log(data.event_insert);
});
```

## UpdateEvent
You can execute the `UpdateEvent` mutation using the following action shortcut function, or by calling `executeMutation()` after calling the following `MutationRef` function, both of which are defined in [dataconnect-generated/index.d.ts](./index.d.ts):
```typescript
updateEvent(): MutationPromise<UpdateEventData, undefined>;

interface UpdateEventRef {
  ...
  /* Allow users to create refs without passing in DataConnect */
  (): MutationRef<UpdateEventData, undefined>;
}
export const updateEventRef: UpdateEventRef;
```
You can also pass in a `DataConnect` instance to the action shortcut function or `MutationRef` function.
```typescript
updateEvent(dc: DataConnect): MutationPromise<UpdateEventData, undefined>;

interface UpdateEventRef {
  ...
  (dc: DataConnect): MutationRef<UpdateEventData, undefined>;
}
export const updateEventRef: UpdateEventRef;
```

If you need the name of the operation without creating a ref, you can retrieve the operation name by calling the `operationName` property on the updateEventRef:
```typescript
const name = updateEventRef.operationName;
console.log(name);
```

### Variables
The `UpdateEvent` mutation has no variables.
### Return Type
Recall that executing the `UpdateEvent` mutation returns a `MutationPromise` that resolves to an object with a `data` property.

The `data` property is an object of type `UpdateEventData`, which is defined in [dataconnect-generated/index.d.ts](./index.d.ts). It has the following fields:
```typescript
export interface UpdateEventData {
  event_update?: Event_Key | null;
}
```
### Using `UpdateEvent`'s action shortcut function

```typescript
import { getDataConnect } from 'firebase/data-connect';
import { connectorConfig, updateEvent } from '@dataconnect/generated';


// Call the `updateEvent()` function to execute the mutation.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await updateEvent();

// You can also pass in a `DataConnect` instance to the action shortcut function.
const dataConnect = getDataConnect(connectorConfig);
const { data } = await updateEvent(dataConnect);

console.log(data.event_update);

// Or, you can use the `Promise` API.
updateEvent().then((response) => {
  const data = response.data;
  console.log(data.event_update);
});
```

### Using `UpdateEvent`'s `MutationRef` function

```typescript
import { getDataConnect, executeMutation } from 'firebase/data-connect';
import { connectorConfig, updateEventRef } from '@dataconnect/generated';


// Call the `updateEventRef()` function to get a reference to the mutation.
const ref = updateEventRef();

// You can also pass in a `DataConnect` instance to the `MutationRef` function.
const dataConnect = getDataConnect(connectorConfig);
const ref = updateEventRef(dataConnect);

// Call `executeMutation()` on the reference to execute the mutation.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await executeMutation(ref);

console.log(data.event_update);

// Or, you can use the `Promise` API.
executeMutation(ref).then((response) => {
  const data = response.data;
  console.log(data.event_update);
});
```

## DeleteEvent
You can execute the `DeleteEvent` mutation using the following action shortcut function, or by calling `executeMutation()` after calling the following `MutationRef` function, both of which are defined in [dataconnect-generated/index.d.ts](./index.d.ts):
```typescript
deleteEvent(): MutationPromise<DeleteEventData, undefined>;

interface DeleteEventRef {
  ...
  /* Allow users to create refs without passing in DataConnect */
  (): MutationRef<DeleteEventData, undefined>;
}
export const deleteEventRef: DeleteEventRef;
```
You can also pass in a `DataConnect` instance to the action shortcut function or `MutationRef` function.
```typescript
deleteEvent(dc: DataConnect): MutationPromise<DeleteEventData, undefined>;

interface DeleteEventRef {
  ...
  (dc: DataConnect): MutationRef<DeleteEventData, undefined>;
}
export const deleteEventRef: DeleteEventRef;
```

If you need the name of the operation without creating a ref, you can retrieve the operation name by calling the `operationName` property on the deleteEventRef:
```typescript
const name = deleteEventRef.operationName;
console.log(name);
```

### Variables
The `DeleteEvent` mutation has no variables.
### Return Type
Recall that executing the `DeleteEvent` mutation returns a `MutationPromise` that resolves to an object with a `data` property.

The `data` property is an object of type `DeleteEventData`, which is defined in [dataconnect-generated/index.d.ts](./index.d.ts). It has the following fields:
```typescript
export interface DeleteEventData {
  event_delete?: Event_Key | null;
}
```
### Using `DeleteEvent`'s action shortcut function

```typescript
import { getDataConnect } from 'firebase/data-connect';
import { connectorConfig, deleteEvent } from '@dataconnect/generated';


// Call the `deleteEvent()` function to execute the mutation.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await deleteEvent();

// You can also pass in a `DataConnect` instance to the action shortcut function.
const dataConnect = getDataConnect(connectorConfig);
const { data } = await deleteEvent(dataConnect);

console.log(data.event_delete);

// Or, you can use the `Promise` API.
deleteEvent().then((response) => {
  const data = response.data;
  console.log(data.event_delete);
});
```

### Using `DeleteEvent`'s `MutationRef` function

```typescript
import { getDataConnect, executeMutation } from 'firebase/data-connect';
import { connectorConfig, deleteEventRef } from '@dataconnect/generated';


// Call the `deleteEventRef()` function to get a reference to the mutation.
const ref = deleteEventRef();

// You can also pass in a `DataConnect` instance to the `MutationRef` function.
const dataConnect = getDataConnect(connectorConfig);
const ref = deleteEventRef(dataConnect);

// Call `executeMutation()` on the reference to execute the mutation.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await executeMutation(ref);

console.log(data.event_delete);

// Or, you can use the `Promise` API.
executeMutation(ref).then((response) => {
  const data = response.data;
  console.log(data.event_delete);
});
```

## CreateSession
You can execute the `CreateSession` mutation using the following action shortcut function, or by calling `executeMutation()` after calling the following `MutationRef` function, both of which are defined in [dataconnect-generated/index.d.ts](./index.d.ts):
```typescript
createSession(): MutationPromise<CreateSessionData, undefined>;

interface CreateSessionRef {
  ...
  /* Allow users to create refs without passing in DataConnect */
  (): MutationRef<CreateSessionData, undefined>;
}
export const createSessionRef: CreateSessionRef;
```
You can also pass in a `DataConnect` instance to the action shortcut function or `MutationRef` function.
```typescript
createSession(dc: DataConnect): MutationPromise<CreateSessionData, undefined>;

interface CreateSessionRef {
  ...
  (dc: DataConnect): MutationRef<CreateSessionData, undefined>;
}
export const createSessionRef: CreateSessionRef;
```

If you need the name of the operation without creating a ref, you can retrieve the operation name by calling the `operationName` property on the createSessionRef:
```typescript
const name = createSessionRef.operationName;
console.log(name);
```

### Variables
The `CreateSession` mutation has no variables.
### Return Type
Recall that executing the `CreateSession` mutation returns a `MutationPromise` that resolves to an object with a `data` property.

The `data` property is an object of type `CreateSessionData`, which is defined in [dataconnect-generated/index.d.ts](./index.d.ts). It has the following fields:
```typescript
export interface CreateSessionData {
  session_insert: Session_Key;
}
```
### Using `CreateSession`'s action shortcut function

```typescript
import { getDataConnect } from 'firebase/data-connect';
import { connectorConfig, createSession } from '@dataconnect/generated';


// Call the `createSession()` function to execute the mutation.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await createSession();

// You can also pass in a `DataConnect` instance to the action shortcut function.
const dataConnect = getDataConnect(connectorConfig);
const { data } = await createSession(dataConnect);

console.log(data.session_insert);

// Or, you can use the `Promise` API.
createSession().then((response) => {
  const data = response.data;
  console.log(data.session_insert);
});
```

### Using `CreateSession`'s `MutationRef` function

```typescript
import { getDataConnect, executeMutation } from 'firebase/data-connect';
import { connectorConfig, createSessionRef } from '@dataconnect/generated';


// Call the `createSessionRef()` function to get a reference to the mutation.
const ref = createSessionRef();

// You can also pass in a `DataConnect` instance to the `MutationRef` function.
const dataConnect = getDataConnect(connectorConfig);
const ref = createSessionRef(dataConnect);

// Call `executeMutation()` on the reference to execute the mutation.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await executeMutation(ref);

console.log(data.session_insert);

// Or, you can use the `Promise` API.
executeMutation(ref).then((response) => {
  const data = response.data;
  console.log(data.session_insert);
});
```

## UpdateSession
You can execute the `UpdateSession` mutation using the following action shortcut function, or by calling `executeMutation()` after calling the following `MutationRef` function, both of which are defined in [dataconnect-generated/index.d.ts](./index.d.ts):
```typescript
updateSession(): MutationPromise<UpdateSessionData, undefined>;

interface UpdateSessionRef {
  ...
  /* Allow users to create refs without passing in DataConnect */
  (): MutationRef<UpdateSessionData, undefined>;
}
export const updateSessionRef: UpdateSessionRef;
```
You can also pass in a `DataConnect` instance to the action shortcut function or `MutationRef` function.
```typescript
updateSession(dc: DataConnect): MutationPromise<UpdateSessionData, undefined>;

interface UpdateSessionRef {
  ...
  (dc: DataConnect): MutationRef<UpdateSessionData, undefined>;
}
export const updateSessionRef: UpdateSessionRef;
```

If you need the name of the operation without creating a ref, you can retrieve the operation name by calling the `operationName` property on the updateSessionRef:
```typescript
const name = updateSessionRef.operationName;
console.log(name);
```

### Variables
The `UpdateSession` mutation has no variables.
### Return Type
Recall that executing the `UpdateSession` mutation returns a `MutationPromise` that resolves to an object with a `data` property.

The `data` property is an object of type `UpdateSessionData`, which is defined in [dataconnect-generated/index.d.ts](./index.d.ts). It has the following fields:
```typescript
export interface UpdateSessionData {
  session_update?: Session_Key | null;
}
```
### Using `UpdateSession`'s action shortcut function

```typescript
import { getDataConnect } from 'firebase/data-connect';
import { connectorConfig, updateSession } from '@dataconnect/generated';


// Call the `updateSession()` function to execute the mutation.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await updateSession();

// You can also pass in a `DataConnect` instance to the action shortcut function.
const dataConnect = getDataConnect(connectorConfig);
const { data } = await updateSession(dataConnect);

console.log(data.session_update);

// Or, you can use the `Promise` API.
updateSession().then((response) => {
  const data = response.data;
  console.log(data.session_update);
});
```

### Using `UpdateSession`'s `MutationRef` function

```typescript
import { getDataConnect, executeMutation } from 'firebase/data-connect';
import { connectorConfig, updateSessionRef } from '@dataconnect/generated';


// Call the `updateSessionRef()` function to get a reference to the mutation.
const ref = updateSessionRef();

// You can also pass in a `DataConnect` instance to the `MutationRef` function.
const dataConnect = getDataConnect(connectorConfig);
const ref = updateSessionRef(dataConnect);

// Call `executeMutation()` on the reference to execute the mutation.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await executeMutation(ref);

console.log(data.session_update);

// Or, you can use the `Promise` API.
executeMutation(ref).then((response) => {
  const data = response.data;
  console.log(data.session_update);
});
```

## DeleteSession
You can execute the `DeleteSession` mutation using the following action shortcut function, or by calling `executeMutation()` after calling the following `MutationRef` function, both of which are defined in [dataconnect-generated/index.d.ts](./index.d.ts):
```typescript
deleteSession(): MutationPromise<DeleteSessionData, undefined>;

interface DeleteSessionRef {
  ...
  /* Allow users to create refs without passing in DataConnect */
  (): MutationRef<DeleteSessionData, undefined>;
}
export const deleteSessionRef: DeleteSessionRef;
```
You can also pass in a `DataConnect` instance to the action shortcut function or `MutationRef` function.
```typescript
deleteSession(dc: DataConnect): MutationPromise<DeleteSessionData, undefined>;

interface DeleteSessionRef {
  ...
  (dc: DataConnect): MutationRef<DeleteSessionData, undefined>;
}
export const deleteSessionRef: DeleteSessionRef;
```

If you need the name of the operation without creating a ref, you can retrieve the operation name by calling the `operationName` property on the deleteSessionRef:
```typescript
const name = deleteSessionRef.operationName;
console.log(name);
```

### Variables
The `DeleteSession` mutation has no variables.
### Return Type
Recall that executing the `DeleteSession` mutation returns a `MutationPromise` that resolves to an object with a `data` property.

The `data` property is an object of type `DeleteSessionData`, which is defined in [dataconnect-generated/index.d.ts](./index.d.ts). It has the following fields:
```typescript
export interface DeleteSessionData {
  session_delete?: Session_Key | null;
}
```
### Using `DeleteSession`'s action shortcut function

```typescript
import { getDataConnect } from 'firebase/data-connect';
import { connectorConfig, deleteSession } from '@dataconnect/generated';


// Call the `deleteSession()` function to execute the mutation.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await deleteSession();

// You can also pass in a `DataConnect` instance to the action shortcut function.
const dataConnect = getDataConnect(connectorConfig);
const { data } = await deleteSession(dataConnect);

console.log(data.session_delete);

// Or, you can use the `Promise` API.
deleteSession().then((response) => {
  const data = response.data;
  console.log(data.session_delete);
});
```

### Using `DeleteSession`'s `MutationRef` function

```typescript
import { getDataConnect, executeMutation } from 'firebase/data-connect';
import { connectorConfig, deleteSessionRef } from '@dataconnect/generated';


// Call the `deleteSessionRef()` function to get a reference to the mutation.
const ref = deleteSessionRef();

// You can also pass in a `DataConnect` instance to the `MutationRef` function.
const dataConnect = getDataConnect(connectorConfig);
const ref = deleteSessionRef(dataConnect);

// Call `executeMutation()` on the reference to execute the mutation.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await executeMutation(ref);

console.log(data.session_delete);

// Or, you can use the `Promise` API.
executeMutation(ref).then((response) => {
  const data = response.data;
  console.log(data.session_delete);
});
```

## CreateObservation
You can execute the `CreateObservation` mutation using the following action shortcut function, or by calling `executeMutation()` after calling the following `MutationRef` function, both of which are defined in [dataconnect-generated/index.d.ts](./index.d.ts):
```typescript
createObservation(): MutationPromise<CreateObservationData, undefined>;

interface CreateObservationRef {
  ...
  /* Allow users to create refs without passing in DataConnect */
  (): MutationRef<CreateObservationData, undefined>;
}
export const createObservationRef: CreateObservationRef;
```
You can also pass in a `DataConnect` instance to the action shortcut function or `MutationRef` function.
```typescript
createObservation(dc: DataConnect): MutationPromise<CreateObservationData, undefined>;

interface CreateObservationRef {
  ...
  (dc: DataConnect): MutationRef<CreateObservationData, undefined>;
}
export const createObservationRef: CreateObservationRef;
```

If you need the name of the operation without creating a ref, you can retrieve the operation name by calling the `operationName` property on the createObservationRef:
```typescript
const name = createObservationRef.operationName;
console.log(name);
```

### Variables
The `CreateObservation` mutation has no variables.
### Return Type
Recall that executing the `CreateObservation` mutation returns a `MutationPromise` that resolves to an object with a `data` property.

The `data` property is an object of type `CreateObservationData`, which is defined in [dataconnect-generated/index.d.ts](./index.d.ts). It has the following fields:
```typescript
export interface CreateObservationData {
  observation_insert: Observation_Key;
}
```
### Using `CreateObservation`'s action shortcut function

```typescript
import { getDataConnect } from 'firebase/data-connect';
import { connectorConfig, createObservation } from '@dataconnect/generated';


// Call the `createObservation()` function to execute the mutation.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await createObservation();

// You can also pass in a `DataConnect` instance to the action shortcut function.
const dataConnect = getDataConnect(connectorConfig);
const { data } = await createObservation(dataConnect);

console.log(data.observation_insert);

// Or, you can use the `Promise` API.
createObservation().then((response) => {
  const data = response.data;
  console.log(data.observation_insert);
});
```

### Using `CreateObservation`'s `MutationRef` function

```typescript
import { getDataConnect, executeMutation } from 'firebase/data-connect';
import { connectorConfig, createObservationRef } from '@dataconnect/generated';


// Call the `createObservationRef()` function to get a reference to the mutation.
const ref = createObservationRef();

// You can also pass in a `DataConnect` instance to the `MutationRef` function.
const dataConnect = getDataConnect(connectorConfig);
const ref = createObservationRef(dataConnect);

// Call `executeMutation()` on the reference to execute the mutation.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await executeMutation(ref);

console.log(data.observation_insert);

// Or, you can use the `Promise` API.
executeMutation(ref).then((response) => {
  const data = response.data;
  console.log(data.observation_insert);
});
```

## UpdateObservation
You can execute the `UpdateObservation` mutation using the following action shortcut function, or by calling `executeMutation()` after calling the following `MutationRef` function, both of which are defined in [dataconnect-generated/index.d.ts](./index.d.ts):
```typescript
updateObservation(): MutationPromise<UpdateObservationData, undefined>;

interface UpdateObservationRef {
  ...
  /* Allow users to create refs without passing in DataConnect */
  (): MutationRef<UpdateObservationData, undefined>;
}
export const updateObservationRef: UpdateObservationRef;
```
You can also pass in a `DataConnect` instance to the action shortcut function or `MutationRef` function.
```typescript
updateObservation(dc: DataConnect): MutationPromise<UpdateObservationData, undefined>;

interface UpdateObservationRef {
  ...
  (dc: DataConnect): MutationRef<UpdateObservationData, undefined>;
}
export const updateObservationRef: UpdateObservationRef;
```

If you need the name of the operation without creating a ref, you can retrieve the operation name by calling the `operationName` property on the updateObservationRef:
```typescript
const name = updateObservationRef.operationName;
console.log(name);
```

### Variables
The `UpdateObservation` mutation has no variables.
### Return Type
Recall that executing the `UpdateObservation` mutation returns a `MutationPromise` that resolves to an object with a `data` property.

The `data` property is an object of type `UpdateObservationData`, which is defined in [dataconnect-generated/index.d.ts](./index.d.ts). It has the following fields:
```typescript
export interface UpdateObservationData {
  observation_update?: Observation_Key | null;
}
```
### Using `UpdateObservation`'s action shortcut function

```typescript
import { getDataConnect } from 'firebase/data-connect';
import { connectorConfig, updateObservation } from '@dataconnect/generated';


// Call the `updateObservation()` function to execute the mutation.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await updateObservation();

// You can also pass in a `DataConnect` instance to the action shortcut function.
const dataConnect = getDataConnect(connectorConfig);
const { data } = await updateObservation(dataConnect);

console.log(data.observation_update);

// Or, you can use the `Promise` API.
updateObservation().then((response) => {
  const data = response.data;
  console.log(data.observation_update);
});
```

### Using `UpdateObservation`'s `MutationRef` function

```typescript
import { getDataConnect, executeMutation } from 'firebase/data-connect';
import { connectorConfig, updateObservationRef } from '@dataconnect/generated';


// Call the `updateObservationRef()` function to get a reference to the mutation.
const ref = updateObservationRef();

// You can also pass in a `DataConnect` instance to the `MutationRef` function.
const dataConnect = getDataConnect(connectorConfig);
const ref = updateObservationRef(dataConnect);

// Call `executeMutation()` on the reference to execute the mutation.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await executeMutation(ref);

console.log(data.observation_update);

// Or, you can use the `Promise` API.
executeMutation(ref).then((response) => {
  const data = response.data;
  console.log(data.observation_update);
});
```

## DeleteObservation
You can execute the `DeleteObservation` mutation using the following action shortcut function, or by calling `executeMutation()` after calling the following `MutationRef` function, both of which are defined in [dataconnect-generated/index.d.ts](./index.d.ts):
```typescript
deleteObservation(): MutationPromise<DeleteObservationData, undefined>;

interface DeleteObservationRef {
  ...
  /* Allow users to create refs without passing in DataConnect */
  (): MutationRef<DeleteObservationData, undefined>;
}
export const deleteObservationRef: DeleteObservationRef;
```
You can also pass in a `DataConnect` instance to the action shortcut function or `MutationRef` function.
```typescript
deleteObservation(dc: DataConnect): MutationPromise<DeleteObservationData, undefined>;

interface DeleteObservationRef {
  ...
  (dc: DataConnect): MutationRef<DeleteObservationData, undefined>;
}
export const deleteObservationRef: DeleteObservationRef;
```

If you need the name of the operation without creating a ref, you can retrieve the operation name by calling the `operationName` property on the deleteObservationRef:
```typescript
const name = deleteObservationRef.operationName;
console.log(name);
```

### Variables
The `DeleteObservation` mutation has no variables.
### Return Type
Recall that executing the `DeleteObservation` mutation returns a `MutationPromise` that resolves to an object with a `data` property.

The `data` property is an object of type `DeleteObservationData`, which is defined in [dataconnect-generated/index.d.ts](./index.d.ts). It has the following fields:
```typescript
export interface DeleteObservationData {
  observation_delete?: Observation_Key | null;
}
```
### Using `DeleteObservation`'s action shortcut function

```typescript
import { getDataConnect } from 'firebase/data-connect';
import { connectorConfig, deleteObservation } from '@dataconnect/generated';


// Call the `deleteObservation()` function to execute the mutation.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await deleteObservation();

// You can also pass in a `DataConnect` instance to the action shortcut function.
const dataConnect = getDataConnect(connectorConfig);
const { data } = await deleteObservation(dataConnect);

console.log(data.observation_delete);

// Or, you can use the `Promise` API.
deleteObservation().then((response) => {
  const data = response.data;
  console.log(data.observation_delete);
});
```

### Using `DeleteObservation`'s `MutationRef` function

```typescript
import { getDataConnect, executeMutation } from 'firebase/data-connect';
import { connectorConfig, deleteObservationRef } from '@dataconnect/generated';


// Call the `deleteObservationRef()` function to get a reference to the mutation.
const ref = deleteObservationRef();

// You can also pass in a `DataConnect` instance to the `MutationRef` function.
const dataConnect = getDataConnect(connectorConfig);
const ref = deleteObservationRef(dataConnect);

// Call `executeMutation()` on the reference to execute the mutation.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await executeMutation(ref);

console.log(data.observation_delete);

// Or, you can use the `Promise` API.
executeMutation(ref).then((response) => {
  const data = response.data;
  console.log(data.observation_delete);
});
```

## CreatePhotoSetting
You can execute the `CreatePhotoSetting` mutation using the following action shortcut function, or by calling `executeMutation()` after calling the following `MutationRef` function, both of which are defined in [dataconnect-generated/index.d.ts](./index.d.ts):
```typescript
createPhotoSetting(): MutationPromise<CreatePhotoSettingData, undefined>;

interface CreatePhotoSettingRef {
  ...
  /* Allow users to create refs without passing in DataConnect */
  (): MutationRef<CreatePhotoSettingData, undefined>;
}
export const createPhotoSettingRef: CreatePhotoSettingRef;
```
You can also pass in a `DataConnect` instance to the action shortcut function or `MutationRef` function.
```typescript
createPhotoSetting(dc: DataConnect): MutationPromise<CreatePhotoSettingData, undefined>;

interface CreatePhotoSettingRef {
  ...
  (dc: DataConnect): MutationRef<CreatePhotoSettingData, undefined>;
}
export const createPhotoSettingRef: CreatePhotoSettingRef;
```

If you need the name of the operation without creating a ref, you can retrieve the operation name by calling the `operationName` property on the createPhotoSettingRef:
```typescript
const name = createPhotoSettingRef.operationName;
console.log(name);
```

### Variables
The `CreatePhotoSetting` mutation has no variables.
### Return Type
Recall that executing the `CreatePhotoSetting` mutation returns a `MutationPromise` that resolves to an object with a `data` property.

The `data` property is an object of type `CreatePhotoSettingData`, which is defined in [dataconnect-generated/index.d.ts](./index.d.ts). It has the following fields:
```typescript
export interface CreatePhotoSettingData {
  photoSetting_insert: PhotoSetting_Key;
}
```
### Using `CreatePhotoSetting`'s action shortcut function

```typescript
import { getDataConnect } from 'firebase/data-connect';
import { connectorConfig, createPhotoSetting } from '@dataconnect/generated';


// Call the `createPhotoSetting()` function to execute the mutation.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await createPhotoSetting();

// You can also pass in a `DataConnect` instance to the action shortcut function.
const dataConnect = getDataConnect(connectorConfig);
const { data } = await createPhotoSetting(dataConnect);

console.log(data.photoSetting_insert);

// Or, you can use the `Promise` API.
createPhotoSetting().then((response) => {
  const data = response.data;
  console.log(data.photoSetting_insert);
});
```

### Using `CreatePhotoSetting`'s `MutationRef` function

```typescript
import { getDataConnect, executeMutation } from 'firebase/data-connect';
import { connectorConfig, createPhotoSettingRef } from '@dataconnect/generated';


// Call the `createPhotoSettingRef()` function to get a reference to the mutation.
const ref = createPhotoSettingRef();

// You can also pass in a `DataConnect` instance to the `MutationRef` function.
const dataConnect = getDataConnect(connectorConfig);
const ref = createPhotoSettingRef(dataConnect);

// Call `executeMutation()` on the reference to execute the mutation.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await executeMutation(ref);

console.log(data.photoSetting_insert);

// Or, you can use the `Promise` API.
executeMutation(ref).then((response) => {
  const data = response.data;
  console.log(data.photoSetting_insert);
});
```

## UpdatePhotoSetting
You can execute the `UpdatePhotoSetting` mutation using the following action shortcut function, or by calling `executeMutation()` after calling the following `MutationRef` function, both of which are defined in [dataconnect-generated/index.d.ts](./index.d.ts):
```typescript
updatePhotoSetting(): MutationPromise<UpdatePhotoSettingData, undefined>;

interface UpdatePhotoSettingRef {
  ...
  /* Allow users to create refs without passing in DataConnect */
  (): MutationRef<UpdatePhotoSettingData, undefined>;
}
export const updatePhotoSettingRef: UpdatePhotoSettingRef;
```
You can also pass in a `DataConnect` instance to the action shortcut function or `MutationRef` function.
```typescript
updatePhotoSetting(dc: DataConnect): MutationPromise<UpdatePhotoSettingData, undefined>;

interface UpdatePhotoSettingRef {
  ...
  (dc: DataConnect): MutationRef<UpdatePhotoSettingData, undefined>;
}
export const updatePhotoSettingRef: UpdatePhotoSettingRef;
```

If you need the name of the operation without creating a ref, you can retrieve the operation name by calling the `operationName` property on the updatePhotoSettingRef:
```typescript
const name = updatePhotoSettingRef.operationName;
console.log(name);
```

### Variables
The `UpdatePhotoSetting` mutation has no variables.
### Return Type
Recall that executing the `UpdatePhotoSetting` mutation returns a `MutationPromise` that resolves to an object with a `data` property.

The `data` property is an object of type `UpdatePhotoSettingData`, which is defined in [dataconnect-generated/index.d.ts](./index.d.ts). It has the following fields:
```typescript
export interface UpdatePhotoSettingData {
  photoSetting_update?: PhotoSetting_Key | null;
}
```
### Using `UpdatePhotoSetting`'s action shortcut function

```typescript
import { getDataConnect } from 'firebase/data-connect';
import { connectorConfig, updatePhotoSetting } from '@dataconnect/generated';


// Call the `updatePhotoSetting()` function to execute the mutation.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await updatePhotoSetting();

// You can also pass in a `DataConnect` instance to the action shortcut function.
const dataConnect = getDataConnect(connectorConfig);
const { data } = await updatePhotoSetting(dataConnect);

console.log(data.photoSetting_update);

// Or, you can use the `Promise` API.
updatePhotoSetting().then((response) => {
  const data = response.data;
  console.log(data.photoSetting_update);
});
```

### Using `UpdatePhotoSetting`'s `MutationRef` function

```typescript
import { getDataConnect, executeMutation } from 'firebase/data-connect';
import { connectorConfig, updatePhotoSettingRef } from '@dataconnect/generated';


// Call the `updatePhotoSettingRef()` function to get a reference to the mutation.
const ref = updatePhotoSettingRef();

// You can also pass in a `DataConnect` instance to the `MutationRef` function.
const dataConnect = getDataConnect(connectorConfig);
const ref = updatePhotoSettingRef(dataConnect);

// Call `executeMutation()` on the reference to execute the mutation.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await executeMutation(ref);

console.log(data.photoSetting_update);

// Or, you can use the `Promise` API.
executeMutation(ref).then((response) => {
  const data = response.data;
  console.log(data.photoSetting_update);
});
```

## DeletePhotoSetting
You can execute the `DeletePhotoSetting` mutation using the following action shortcut function, or by calling `executeMutation()` after calling the following `MutationRef` function, both of which are defined in [dataconnect-generated/index.d.ts](./index.d.ts):
```typescript
deletePhotoSetting(): MutationPromise<DeletePhotoSettingData, undefined>;

interface DeletePhotoSettingRef {
  ...
  /* Allow users to create refs without passing in DataConnect */
  (): MutationRef<DeletePhotoSettingData, undefined>;
}
export const deletePhotoSettingRef: DeletePhotoSettingRef;
```
You can also pass in a `DataConnect` instance to the action shortcut function or `MutationRef` function.
```typescript
deletePhotoSetting(dc: DataConnect): MutationPromise<DeletePhotoSettingData, undefined>;

interface DeletePhotoSettingRef {
  ...
  (dc: DataConnect): MutationRef<DeletePhotoSettingData, undefined>;
}
export const deletePhotoSettingRef: DeletePhotoSettingRef;
```

If you need the name of the operation without creating a ref, you can retrieve the operation name by calling the `operationName` property on the deletePhotoSettingRef:
```typescript
const name = deletePhotoSettingRef.operationName;
console.log(name);
```

### Variables
The `DeletePhotoSetting` mutation has no variables.
### Return Type
Recall that executing the `DeletePhotoSetting` mutation returns a `MutationPromise` that resolves to an object with a `data` property.

The `data` property is an object of type `DeletePhotoSettingData`, which is defined in [dataconnect-generated/index.d.ts](./index.d.ts). It has the following fields:
```typescript
export interface DeletePhotoSettingData {
  photoSetting_delete?: PhotoSetting_Key | null;
}
```
### Using `DeletePhotoSetting`'s action shortcut function

```typescript
import { getDataConnect } from 'firebase/data-connect';
import { connectorConfig, deletePhotoSetting } from '@dataconnect/generated';


// Call the `deletePhotoSetting()` function to execute the mutation.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await deletePhotoSetting();

// You can also pass in a `DataConnect` instance to the action shortcut function.
const dataConnect = getDataConnect(connectorConfig);
const { data } = await deletePhotoSetting(dataConnect);

console.log(data.photoSetting_delete);

// Or, you can use the `Promise` API.
deletePhotoSetting().then((response) => {
  const data = response.data;
  console.log(data.photoSetting_delete);
});
```

### Using `DeletePhotoSetting`'s `MutationRef` function

```typescript
import { getDataConnect, executeMutation } from 'firebase/data-connect';
import { connectorConfig, deletePhotoSettingRef } from '@dataconnect/generated';


// Call the `deletePhotoSettingRef()` function to get a reference to the mutation.
const ref = deletePhotoSettingRef();

// You can also pass in a `DataConnect` instance to the `MutationRef` function.
const dataConnect = getDataConnect(connectorConfig);
const ref = deletePhotoSettingRef(dataConnect);

// Call `executeMutation()` on the reference to execute the mutation.
// You can use the `await` keyword to wait for the promise to resolve.
const { data } = await executeMutation(ref);

console.log(data.photoSetting_delete);

// Or, you can use the `Promise` API.
executeMutation(ref).then((response) => {
  const data = response.data;
  console.log(data.photoSetting_delete);
});
```

